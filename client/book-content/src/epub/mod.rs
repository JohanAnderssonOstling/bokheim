use crate::profile::{PreparationProfile, ResourcePreparation};
use crate::resource_path::resolve_resource_href;
use crate::resource_path::{is_container_resource, normalize_path as normalize_epub_path};
use crate::resources::{MAX_BINARY_RESOURCE_BYTES, MAX_PREPARED_TEXT_BYTES};
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, OnceLock};
use std::time::Instant;

pub(crate) mod archive;
use archive::{Archive, ArchiveMetadata, ArchiveReader};
pub(crate) use inspection::EpubSession;
mod inspection;
pub(crate) mod opening;

const MAX_CONTAINER_XML_BYTES: u64 = 1024 * 1024;
const MAX_PACKAGE_DOCUMENT_BYTES: u64 = 8 * 1024 * 1024;
const MAX_MARKUP_RESOURCE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 65_536;

use crate::TocEntry;

use crate::metadata::*;

#[derive(Clone, Debug)]
struct ManifestItem {
    id: String,
    href: String,
    media_type: Option<String>,
    properties: Option<String>,
}

#[derive(Clone, Debug)]
struct ParsedManifest {
    items: Vec<ManifestItem>,
    spine: Vec<String>,
    nav_href: Option<String>,
    ncx_href: Option<String>,
}

/// An immutable async source for interactive reading.
pub(crate) struct EpubProvider {
    archive: Archive,
    pub(crate) package: Arc<EpubPackage>,
}

/// Parsed package and directory metadata shared by owned inspection sessions
/// and asynchronous reading sources.
pub(crate) struct EpubPackage {
    archive: Arc<ArchiveMetadata>,
    opf_path: String,
    opf_xml: String,
    manifest_cache: OnceLock<Arc<ParsedManifest>>,
}

impl EpubPackage {
    async fn open(reader: &mut ArchiveReader) -> io::Result<Self> {
        let container = reader.read("META-INF/container.xml", MAX_CONTAINER_XML_BYTES).await?;
        let opf_path = Self::parse_container_opf_path(&String::from_utf8_lossy(&container)).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Missing OPF path in container.xml"))?;
        let opf = reader.read(&opf_path, MAX_PACKAGE_DOCUMENT_BYTES).await?;
        Ok(Self { archive: reader.metadata.clone(), opf_path, opf_xml: String::from_utf8_lossy(&opf).into_owned(), manifest_cache: OnceLock::new() })
    }

    /// Returns the package-declared media type for an archive resource.
    /// EPUB content type comes from the manifest, not the resource filename.
    #[cfg(test)]
    fn resource_media_type(&self, uri: &str) -> Option<&str> {
        let path = normalize_epub_path(uri.split(['?', '#']).next().unwrap_or(uri));
        self.get_or_parse_manifest().items.iter().find(|item| is_container_resource(&item.href) && resolve_resource_href(&self.opf_path, &item.href) == path).and_then(|item| item.media_type.as_deref())
    }

    /// Snapshot container-local manifest metadata without retaining the archive.
    pub(crate) fn resource_media_types(&self) -> HashMap<String, String> {
        self.get_or_parse_manifest()
            .items
            .iter()
            .filter(|item| is_container_resource(&item.href))
            .filter_map(|item| item.media_type.as_ref().map(|media_type| (resolve_resource_href(&self.opf_path, &item.href), media_type.clone())))
            .collect()
    }

    /// Progress weights come from immutable ZIP directory metadata.
    pub(crate) fn document_progress_weights(&self, documents: &[String]) -> io::Result<Vec<u64>> {
        documents.iter().map(|uri| self.archive.size(uri).map(|size| size.max(1))).collect()
    }

    fn exists(&self, uri: &str) -> bool {
        self.archive.contains(uri)
    }

    fn is_html_entry(name: &str) -> bool {
        let len = name.len();
        if len >= 5 && name[len - 5..].eq_ignore_ascii_case(".html") {
            return true;
        }
        if len >= 6 && name[len - 6..].eq_ignore_ascii_case(".xhtml") {
            return true;
        }
        if len >= 4 && name[len - 4..].eq_ignore_ascii_case(".htm") {
            return true;
        }
        if len >= 4 && name[len - 4..].eq_ignore_ascii_case(".svg") {
            return true;
        }
        false
    }

    fn parse_container_opf_path(container_xml: &str) -> Option<String> {
        let mut reader = quick_xml::Reader::from_str(container_xml);
        reader.trim_text(true);
        let mut buf = Vec::new();
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Empty(e)) | Ok(quick_xml::events::Event::Start(e)) if e.name().as_ref().ends_with(b"rootfile") => {
                    return read_xml_attr(e.attributes(), b"full-path");
                }
                Ok(quick_xml::events::Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }
        None
    }

    fn get_or_parse_manifest(&self) -> &Arc<ParsedManifest> {
        self.manifest_cache.get_or_init(|| Arc::new(Self::parse_opf_manifest(&self.opf_xml)))
    }

    fn parse_opf_manifest(opf_xml: &str) -> ParsedManifest {
        let mut reader = quick_xml::Reader::from_str(opf_xml);
        reader.trim_text(true);
        let mut buf = Vec::new();
        let mut items = Vec::new();
        let mut spine = Vec::new();
        let mut nav_href: Option<String> = None;
        let mut toc_id: Option<String> = None;
        let mut in_manifest = false;
        let mut in_spine = false;

        loop {
            match reader.read_event_into(&mut buf) {
                Ok(quick_xml::events::Event::Start(e)) | Ok(quick_xml::events::Event::Empty(e)) => {
                    if e.name().as_ref().ends_with(b"manifest") {
                        in_manifest = true;
                    } else if e.name().as_ref().ends_with(b"spine") {
                        in_spine = true;
                        toc_id = read_xml_attr(e.attributes(), b"toc");
                    } else if in_manifest && e.name().as_ref().ends_with(b"item") {
                        let mut id = None;
                        let mut href = None;
                        let mut media_type = None;
                        let mut properties = None;

                        for attr in e.attributes().flatten() {
                            let key = attr.key.as_ref();
                            if key.ends_with(b"id") {
                                id = Some(String::from_utf8_lossy(&attr.value).into_owned());
                            } else if key.ends_with(b"href") {
                                href = Some(String::from_utf8_lossy(&attr.value).into_owned());
                            } else if key.ends_with(b"media-type") {
                                media_type = Some(String::from_utf8_lossy(&attr.value).into_owned());
                            } else if key.ends_with(b"properties") {
                                properties = Some(String::from_utf8_lossy(&attr.value).into_owned());
                            }
                        }

                        if let (Some(id), Some(href)) = (id, href) {
                            if let Some(ref props) = properties
                                && props.split_whitespace().any(|v| v == "nav")
                            {
                                nav_href = Some(href.clone());
                            }
                            items.push(ManifestItem { id, href, media_type, properties });
                        }
                    } else if in_spine
                        && e.name().as_ref().ends_with(b"itemref")
                        && let Some(idref) = read_xml_attr(e.attributes(), b"idref")
                    {
                        spine.push(idref);
                    }
                }
                Ok(quick_xml::events::Event::End(e)) => {
                    if e.name().as_ref().ends_with(b"manifest") {
                        in_manifest = false;
                    } else if e.name().as_ref().ends_with(b"spine") {
                        in_spine = false;
                    }
                }
                Ok(quick_xml::events::Event::Eof) => break,
                Err(_) => break,
                _ => {}
            }
            buf.clear();
        }

        let ncx_href = toc_id.and_then(|id| items.iter().find(|item| item.id == id).map(|item| item.href.clone()));

        ParsedManifest { items, spine, nav_href, ncx_href }
    }

    fn resolve_spine_entries(&self) -> io::Result<Option<Vec<String>>> {
        let manifest = self.get_or_parse_manifest();

        if manifest.spine.is_empty() {
            return Ok(None);
        }

        let mut out = Vec::new();

        for idref in &manifest.spine {
            let Some(item) = manifest.items.iter().find(|item| &item.id == idref) else {
                continue;
            };
            let normalized = resolve_resource_href(&self.opf_path, &item.href);
            if matches!(item.media_type.as_deref(), Some("application/xhtml+xml" | "text/html")) || (item.media_type.is_none() && Self::is_html_entry(&normalized)) {
                out.push(normalized);
            }
        }

        if out.is_empty() { Ok(None) } else { Ok(Some(out)) }
    }

    /// Returns the documents the reader can present, preferring the package
    /// spine and falling back to HTML archive entries for older books.
    pub(crate) fn document_uris(&self) -> io::Result<Vec<String>> {
        if let Some(spine) = self.resolve_spine_entries()? {
            return Ok(spine);
        }

        Ok(self.archive.names().iter().filter(|name| Self::is_html_entry(name)).cloned().collect())
    }
}

fn read_xml_attr(attrs: quick_xml::events::attributes::Attributes, key: &[u8]) -> Option<String> {
    attrs.flatten().find(|a| a.key.as_ref().ends_with(key)).map(|a| a.unescape_value().map_or_else(|_| String::from_utf8_lossy(&a.value).into_owned(), std::borrow::Cow::into_owned))
}

impl EpubProvider {
    pub(crate) fn languages(&self) -> Vec<String> {
        inspection::parse_opf_metadata(&self.package.opf_xml).languages
    }

    /// Load a lazy resource using its own cursor and async decompressor.
    pub(crate) async fn read_bytes_async(&self, uri: &str) -> io::Result<Vec<u8>> {
        self.archive.read(uri, MAX_BINARY_RESOURCE_BYTES).await
    }

    pub(crate) async fn prepare_text_resources_async<const PROFILE: bool>(&self, documents: &[String], profile: &mut PreparationProfile) -> io::Result<HashMap<String, Vec<u8>>> {
        let started = PROFILE.then(Instant::now);
        let mut paths = HashMap::new();
        for item in &self.package.get_or_parse_manifest().items {
            if !is_container_resource(&item.href) {
                continue;
            }
            let limit = match item.media_type.as_deref() {
                Some(media_type) if media_type.eq_ignore_ascii_case("text/css") => MAX_MARKUP_RESOURCE_BYTES,
                Some(media_type) if media_type.eq_ignore_ascii_case("application/xhtml+xml") || media_type.eq_ignore_ascii_case("text/html") => MAX_BINARY_RESOURCE_BYTES,
                _ => continue,
            };
            paths.insert(resolve_resource_href(&self.package.opf_path, &item.href), limit);
        }
        for uri in documents {
            if !is_container_resource(uri) {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "external EPUB documents are unsupported"));
            }
            paths.entry(normalize_epub_path(uri)).or_insert(MAX_BINARY_RESOURCE_BYTES);
        }
        profile.record_sync("resource_plan", started);
        let mut resources = HashMap::new();
        let mut total = 0;
        let mut reader = self.archive.reader();
        for (path, limit) in paths {
            let (bytes, timing) = reader.read_measured::<PROFILE>(&path, limit.min(MAX_PREPARED_TEXT_BYTES - total)).await?;
            total += bytes.len() as u64;
            if PROFILE {
                profile.resources.push(ResourcePreparation { uri: path.clone(), timing });
            }
            resources.insert(path, bytes);
        }
        Ok(resources)
    }
}

#[cfg(test)]
mod streaming_tests {
    use super::*;
    use std::io::{Cursor, Read, Seek, SeekFrom, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingReader {
        cursor: Cursor<Vec<u8>>,
        bytes_read: Arc<AtomicUsize>,
    }

    impl Read for CountingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let count = self.cursor.read(buffer)?;
            self.bytes_read.fetch_add(count, Ordering::Relaxed);
            Ok(count)
        }
    }

    impl Seek for CountingReader {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.cursor.seek(position)
        }
    }

    // Build a tiny EPUB and replace only central-directory extra fields.
    // No user book or copyrighted contents are embedded in the fixture.
    fn zip64_epub(sentinels: u8) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, text) in [
            ("META-INF/container.xml", r#"<container><rootfiles><rootfile full-path="OPS/book.opf"/></rootfiles></container>"#),
            ("OPS/book.opf", r#"<package><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#),
            ("OPS/chapter.xhtml", "<html><body>ZIP64 regression chapter</body></html>"),
        ] {
            writer.start_file(name, stored).unwrap();
            writer.write_all(text.as_bytes()).unwrap();
        }
        let bytes = writer.finish().unwrap().into_inner();
        let end = bytes.len() - 22;
        assert_eq!(&bytes[end..end + 4], b"PK\x05\x06");
        let start = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
        let mut output = bytes[..start].to_vec();
        let mut cursor = start;
        while cursor < end {
            assert_eq!(&bytes[cursor..cursor + 4], b"PK\x01\x02");
            let name_len = u16::from_le_bytes(bytes[cursor + 28..cursor + 30].try_into().unwrap()) as usize;
            let extra_len = u16::from_le_bytes(bytes[cursor + 30..cursor + 32].try_into().unwrap()) as usize;
            let comment_len = u16::from_le_bytes(bytes[cursor + 32..cursor + 34].try_into().unwrap()) as usize;
            let mut header = bytes[cursor..cursor + 46].to_vec();
            let mut payload = Vec::new();
            for (mask, offset) in [(1, 24), (2, 20), (4, 42)] {
                if sentinels & mask != 0 {
                    let value = u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap());
                    payload.extend_from_slice(&(value as u64).to_le_bytes());
                    header[offset..offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
                }
            }
            if sentinels == 0 {
                // Broken redundant data must not replace valid sizes/offsets.
                payload.resize(32, 0xff);
            }
            header[30..32].copy_from_slice(&((payload.len() + 4) as u16).to_le_bytes());
            output.extend_from_slice(&header);
            output.extend_from_slice(&bytes[cursor + 46..cursor + 46 + name_len]);
            output.extend_from_slice(&1u16.to_le_bytes());
            output.extend_from_slice(&(payload.len() as u16).to_le_bytes());
            output.extend_from_slice(&payload);
            output.extend_from_slice(&bytes[cursor + 46 + name_len + extra_len..cursor + 46 + name_len + extra_len + comment_len]);
            cursor += 46 + name_len + extra_len + comment_len;
        }
        let size = (output.len() - start) as u32;
        let mut footer = bytes[end..].to_vec();
        footer[12..16].copy_from_slice(&size.to_le_bytes());
        output.extend_from_slice(&footer);
        output
    }

    #[test]
    fn zip64_redundant_fields_preserve_valid_offsets_and_sizes() {
        let mut provider = EpubSession::try_from_reader(Cursor::new(zip64_epub(0))).unwrap();
        assert_eq!(provider.package.document_uris().unwrap(), vec!["OPS/chapter.xhtml"]);
        assert!(provider.read_string("OPS/chapter.xhtml").unwrap().contains("ZIP64 regression chapter"));
    }

    #[test]
    fn zip64_sentinel_fields_still_use_extended_values() {
        for mask in 1..=7 {
            let mut provider = EpubSession::try_from_reader(Cursor::new(zip64_epub(mask))).unwrap();
            assert!(provider.read_string("OPS/chapter.xhtml").unwrap().contains("ZIP64 regression chapter"), "sentinel mask {mask}");
        }
    }

    #[test]
    fn zip64_recovery_does_not_disable_crc_validation() {
        let mut bytes = zip64_epub(0);
        let position = bytes.windows(b"ZIP64 regression chapter".len()).position(|window| window == b"ZIP64 regression chapter").unwrap();
        bytes[position] ^= 1;
        let mut provider = EpubSession::try_from_reader(Cursor::new(bytes)).unwrap();
        assert!(provider.read_string("OPS/chapter.xhtml").is_err());
    }

    #[test]
    fn spine_uses_declared_xhtml_type_for_xml_filenames() {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in [
            ("META-INF/container.xml", "<container><rootfiles><rootfile full-path=\"OPS/book.opf\"/></rootfiles></container>"),
            (
                "OPS/book.opf",
                r#"<package><manifest><item id="chapter" href="chapter.xml" media-type="application/xhtml+xml"/><item id="image" href="image.svg" media-type="image/svg+xml"/></manifest><spine><itemref idref="chapter"/><itemref idref="image"/></spine></package>"#,
            ),
            ("OPS/chapter.xml", "<html><body>Chapter</body></html>"),
        ] {
            archive.start_file(name, options).unwrap();
            archive.write_all(bytes.as_bytes()).unwrap();
        }
        let mut provider = EpubSession::try_from_reader(archive.finish().unwrap()).unwrap();
        assert_eq!(provider.package.document_uris().unwrap(), vec!["OPS/chapter.xml"]);
        assert!(provider.read_string("OPS/chapter.xml").unwrap().contains("Chapter"));
    }

    #[test]
    fn manifest_type_is_available_for_xhtml_with_html_filename() {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in [
            ("META-INF/container.xml", "<container><rootfiles><rootfile full-path=\"OPS/book.opf\"/></rootfiles></container>"),
            ("OPS/book.opf", r#"<package><manifest><item id="chapter" href="Text/chapter.html" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#),
            ("OPS/Text/chapter.html", "<html xmlns=\"http://www.w3.org/1999/xhtml\"><body><a id=\"page1\"/>Text</body></html>"),
        ] {
            archive.start_file(name, options).unwrap();
            archive.write_all(bytes.as_bytes()).unwrap();
        }
        let provider = EpubSession::try_from_reader(archive.finish().unwrap()).unwrap();
        assert_eq!(provider.package.document_uris().unwrap(), vec!["OPS/Text/chapter.html"]);
        assert_eq!(provider.package.resource_media_type("OPS/Text/chapter.html"), Some("application/xhtml+xml"));
    }

    #[test]
    fn opening_and_reading_a_chapter_does_not_read_the_whole_epub() {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        archive.start_file("mimetype", stored).unwrap();
        archive.write_all(b"application/epub+zip").unwrap();
        archive.start_file("META-INF/container.xml", stored).unwrap();
        archive.write_all(br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#).unwrap();
        archive.start_file("OEBPS/content.opf", stored).unwrap();
        archive
            .write_all(br#"<package><metadata><dc:title xmlns:dc="http://purl.org/dc/elements/1.1/">Test</dc:title></metadata><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/><item id="notes" href="notes.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/><itemref idref="notes" linear="no"/></spine></package>"#)
            .unwrap();
        archive.start_file("OEBPS/chapter.xhtml", stored).unwrap();
        archive.write_all(b"<html><body>Only this chapter</body></html>").unwrap();
        archive.start_file("OEBPS/notes.xhtml", stored).unwrap();
        archive.write_all(b"<html><body>Auxiliary notes</body></html>").unwrap();
        archive.start_file("OEBPS/unused.bin", stored).unwrap();
        archive.write_all(&vec![7_u8; 4 * 1024 * 1024]).unwrap();
        let bytes = archive.finish().unwrap().into_inner();
        let bytes_read = Arc::new(AtomicUsize::new(0));
        let mut provider = EpubSession::try_from_reader(CountingReader { cursor: Cursor::new(bytes.clone()), bytes_read: Arc::clone(&bytes_read) }).unwrap();
        let documents = provider.package.document_uris().unwrap();
        assert_eq!(documents, vec!["OEBPS/chapter.xhtml", "OEBPS/notes.xhtml"]);
        assert_eq!(provider.package.document_progress_weights(&documents).unwrap(), vec![43, 41]);
        assert_eq!(provider.read_string("OEBPS/chapter.xhtml").unwrap(), "<html><body>Only this chapter</body></html>");
        assert!(bytes_read.load(Ordering::Relaxed) < bytes.len() / 2, "EPUB provider read the unused archive payload");
    }
}
