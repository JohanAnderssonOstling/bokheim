//! Shared, deterministic first-line validation for book uploads.
//!
//! Clients use this policy to avoid uploading obviously invalid files. Servers
//! must run the same validation again: a client result is never authoritative.

use image::io::Reader as ImageReader;
use image::ImageFormat;
use lopdf::{Dictionary, Document, Object};
use percent_encoding::percent_decode_str;
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use zip::CompressionMethod;
use zip::ZipArchive;

pub const MAX_BOOK_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_EPUB_ENTRIES: usize = 20_000;
const MAX_EPUB_UNCOMPRESSED_BYTES: u64 = 2 * MAX_BOOK_BYTES;
const MAX_RESOURCE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_XML_BYTES: u64 = 16 * 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMAGE_DIMENSION: u32 = 20_000;
const MAX_IMAGE_PIXELS: u64 = 100_000_000;
const MAX_PDF_OBJECTS: usize = 250_000;
const MAX_PDF_PAGES: usize = 100_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BookFormat {
    Epub,
    Pdf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidatedBook {
    pub format: BookFormat,
    pub size: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("book cannot be read: {0}")]
    Io(#[from] std::io::Error),
    #[error("book exceeds the {limit}-byte limit")]
    TooLarge { limit: u64 },
    #[error("unsupported top-level book format; only EPUB and PDF are accepted")]
    UnsupportedFormat,
    #[error("invalid EPUB: {0}")]
    InvalidEpub(String),
    #[error("invalid or unsafe PDF: {0}")]
    InvalidPdf(String),
}

/// Validates the bytes at `path`; the file extension and caller-supplied MIME
/// type deliberately do not participate in format detection.
pub fn validate_path(path: impl AsRef<Path>) -> Result<ValidatedBook, ValidationError> {
    let path = path.as_ref();
    let size = std::fs::metadata(path)?.len();
    if size > MAX_BOOK_BYTES {
        return Err(ValidationError::TooLarge { limit: MAX_BOOK_BYTES });
    }

    let mut file = File::open(path)?;
    let mut signature = [0_u8; 8];
    let count = file.read(&mut signature)?;
    file.seek(SeekFrom::Start(0))?;

    let format = if count >= 4 && matches!(&signature[..4], b"PK\x03\x04" | b"PK\x05\x06" | b"PK\x07\x08") {
        validate_epub(file)?;
        BookFormat::Epub
    } else if count >= 5 && &signature[..5] == b"%PDF-" {
        validate_pdf(path)?;
        BookFormat::Pdf
    } else {
        return Err(ValidationError::UnsupportedFormat);
    };

    Ok(ValidatedBook { format, size })
}

#[derive(Debug)]
struct ManifestItem {
    path: String,
    media_type: String,
    properties: BTreeSet<String>,
}

fn validate_epub(file: File) -> Result<(), ValidationError> {
    let mut archive = ZipArchive::new(file).map_err(epub)?;
    if archive.is_empty() || archive.len() > MAX_EPUB_ENTRIES {
        return Err(epub(format!("archive entry count {} is outside the supported range", archive.len())));
    }

    let mut archive_files = BTreeMap::<String, usize>::new();
    let mut uncompressed_total = 0_u64;
    for index in 0..archive.len() {
        let raw_entry = archive.by_index_raw(index).map_err(epub)?;
        if raw_entry.encrypted() {
            return Err(epub(format!("encrypted ZIP entry '{}' is not accepted", raw_entry.name())));
        }
        drop(raw_entry);
        let entry = archive.by_index(index).map_err(epub)?;
        let raw_name = entry.name();
        let name = canonical_archive_path(raw_name).map_err(epub)?;
        if entry.is_dir() {
            continue;
        }
        if archive_files.insert(name.clone(), index).is_some() {
            return Err(epub(format!("duplicate archive path '{name}'")));
        }
        if entry.size() > MAX_RESOURCE_BYTES {
            return Err(epub(format!("resource '{name}' exceeds the per-resource limit")));
        }
        uncompressed_total = uncompressed_total.checked_add(entry.size()).ok_or_else(|| epub("uncompressed size overflow"))?;
        if uncompressed_total > MAX_EPUB_UNCOMPRESSED_BYTES {
            return Err(epub("total uncompressed size exceeds the EPUB limit"));
        }
        if entry.compressed_size() > 0 && entry.size() > entry.compressed_size().saturating_mul(200) {
            return Err(epub(format!("resource '{name}' has an unsafe compression ratio")));
        }
    }

    let first = archive.by_index(0).map_err(epub)?;
    if first.name() != "mimetype" || first.compression() != CompressionMethod::Stored {
        return Err(epub("the first ZIP entry must be the uncompressed 'mimetype' file"));
    }
    drop(first);
    let mimetype = read_entry(&mut archive, "mimetype", 64)?;
    if mimetype != b"application/epub+zip" {
        return Err(epub("the mimetype file must contain exactly 'application/epub+zip'"));
    }

    let container = read_entry(&mut archive, "META-INF/container.xml", MAX_XML_BYTES)?;
    let opf_path = parse_container(&container)?;
    if !archive_files.contains_key(&opf_path) {
        return Err(epub(format!("package document '{opf_path}' is missing")));
    }
    let opf = read_entry(&mut archive, &opf_path, MAX_XML_BYTES)?;
    let manifest = parse_package(&opf, &opf_path)?;
    if manifest.is_empty() {
        return Err(epub("package manifest is empty"));
    }

    let manifest_paths: BTreeSet<_> = manifest.keys().cloned().collect();
    let actual_publication_paths: BTreeSet<_> = archive_files.keys().filter(|path| path.as_str() != "mimetype" && path.as_str() != "META-INF/container.xml" && path.as_str() != opf_path).cloned().collect();

    if archive_files.contains_key("META-INF/encryption.xml") {
        return Err(epub("encrypted EPUB resources are not accepted"));
    }
    if let Some(path) = archive_files.keys().find(|path| path.starts_with("META-INF/") && path.as_str() != "META-INF/container.xml") {
        return Err(epub(format!("unsupported EPUB control file '{path}'")));
    }
    if let Some(path) = actual_publication_paths.difference(&manifest_paths).next() {
        return Err(epub(format!("archive resource '{path}' is not declared in the package manifest")));
    }
    if let Some(path) = manifest_paths.difference(&actual_publication_paths).next() {
        return Err(epub(format!("manifest resource '{path}' is missing from the archive")));
    }

    for item in manifest.values() {
        validate_manifest_resource(&mut archive, item)?;
    }
    Ok(())
}

fn parse_container(bytes: &[u8]) -> Result<String, ValidationError> {
    let mut reader = Reader::from_reader(bytes);
    reader.trim_text(true);
    let mut buffer = Vec::new();
    let mut rootfiles = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer).map_err(epub)? {
            Event::Start(element) | Event::Empty(element) if local_name(element.name().as_ref()) == b"rootfile" => {
                let path = required_attr(&element, b"full-path")?;
                let media_type = required_attr(&element, b"media-type")?;
                if media_type != "application/oebps-package+xml" {
                    return Err(epub("rootfile has an unsupported media type"));
                }
                rootfiles.push(canonical_archive_path(&path).map_err(epub)?);
            }
            Event::DocType(_) => return Err(epub("XML document types are not allowed")),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if rootfiles.len() != 1 {
        return Err(epub(format!("container must identify exactly one package document, found {}", rootfiles.len())));
    }
    let path = rootfiles.pop().expect("one rootfile was checked");
    if path == "mimetype" || path.starts_with("META-INF/") {
        return Err(epub("package document uses a reserved path"));
    }
    Ok(path)
}

fn parse_package(bytes: &[u8], opf_path: &str) -> Result<BTreeMap<String, ManifestItem>, ValidationError> {
    let mut reader = Reader::from_reader(bytes);
    reader.trim_text(true);
    let mut buffer = Vec::new();
    let mut manifest_depth = None;
    let mut depth = 0_usize;
    let mut ids = BTreeSet::new();
    let mut paths = BTreeMap::new();
    loop {
        match reader.read_event_into(&mut buffer).map_err(epub)? {
            Event::Start(element) => {
                depth += 1;
                if local_name(element.name().as_ref()) == b"manifest" {
                    manifest_depth = Some(depth);
                } else if manifest_depth.is_some() && local_name(element.name().as_ref()) == b"item" {
                    insert_manifest_item(&element, opf_path, &mut ids, &mut paths)?;
                }
            }
            Event::Empty(element) if manifest_depth.is_some() && local_name(element.name().as_ref()) == b"item" => {
                insert_manifest_item(&element, opf_path, &mut ids, &mut paths)?;
            }
            Event::End(element) => {
                if local_name(element.name().as_ref()) == b"manifest" && manifest_depth == Some(depth) {
                    manifest_depth = None;
                }
                depth = depth.saturating_sub(1);
            }
            Event::DocType(_) => return Err(epub("XML document types are not allowed")),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(paths)
}

fn insert_manifest_item(element: &BytesStart<'_>, opf_path: &str, ids: &mut BTreeSet<String>, paths: &mut BTreeMap<String, ManifestItem>) -> Result<(), ValidationError> {
    let id = required_attr(element, b"id")?;
    if !ids.insert(id.clone()) {
        return Err(epub(format!("duplicate manifest id '{id}'")));
    }
    let href = required_attr(element, b"href")?;
    let path = resolve_manifest_path(opf_path, &href).map_err(epub)?;
    let media_type = required_attr(element, b"media-type")?.to_ascii_lowercase();
    let properties = optional_attr(element, b"properties")?.unwrap_or_default().split_whitespace().map(str::to_owned).collect();
    let item = ManifestItem { path: path.clone(), media_type, properties };
    if paths.insert(path.clone(), item).is_some() {
        return Err(epub(format!("multiple manifest items resolve to '{path}'")));
    }
    Ok(())
}

fn validate_manifest_resource(archive: &mut ZipArchive<File>, item: &ManifestItem) -> Result<(), ValidationError> {
    if item.properties.contains("scripted") {
        return Err(epub(format!("manifest resource '{}' declares prohibited scripting", item.path)));
    }
    match item.media_type.as_str() {
        "image/jpeg" => validate_image(archive, item, ImageFormat::Jpeg),
        "image/png" => validate_image(archive, item, ImageFormat::Png),
        "image/gif" => validate_image(archive, item, ImageFormat::Gif),
        "image/webp" => validate_image(archive, item, ImageFormat::WebP),
        "application/xhtml+xml" | "application/x-dtbncx+xml" => {
            let bytes = read_entry(archive, &item.path, MAX_XML_BYTES)?;
            validate_content_xml(&bytes, &item.path)
        }
        "text/css" => {
            let bytes = read_entry(archive, &item.path, MAX_XML_BYTES)?;
            let css = std::str::from_utf8(&bytes).map_err(|_| epub(format!("CSS resource '{}' is not UTF-8", item.path)))?;
            let lowered = css.to_ascii_lowercase();
            if ["url(data:", "url(http:", "url(https:", "url(file:", "@import url(http:", "@import url(https:"].iter().any(|needle| lowered.contains(needle)) {
                return Err(epub(format!("CSS resource '{}' contains a remote or embedded resource", item.path)));
            }
            Ok(())
        }
        "font/otf" | "application/vnd.ms-opentype" => validate_font(archive, item, &[b"OTTO"]),
        "font/ttf" | "application/font-sfnt" => validate_font(archive, item, &[b"\0\x01\0\0", b"true"]),
        "font/woff" | "application/font-woff" => validate_font(archive, item, &[b"wOFF"]),
        "font/woff2" => validate_font(archive, item, &[b"wOF2"]),
        "application/xml" | "text/xml" => {
            let bytes = read_entry(archive, &item.path, MAX_XML_BYTES)?;
            validate_content_xml(&bytes, &item.path)
        }
        media_type => Err(epub(format!("manifest resource '{}' uses prohibited or unsupported media type '{media_type}'", item.path))),
    }
}

fn validate_image(archive: &mut ZipArchive<File>, item: &ManifestItem, expected: ImageFormat) -> Result<(), ValidationError> {
    let bytes = read_entry(archive, &item.path, MAX_IMAGE_BYTES)?;
    let actual = image::guess_format(&bytes).map_err(|error| epub(format!("image '{}' is invalid: {error}", item.path)))?;
    if actual != expected {
        return Err(epub(format!("image '{}' bytes do not match manifest media type '{}'", item.path, item.media_type)));
    }
    let (width, height) = ImageReader::with_format(Cursor::new(&bytes), actual).into_dimensions().map_err(|error| epub(format!("image '{}' dimensions cannot be read: {error}", item.path)))?;
    if width == 0 || height == 0 || width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(epub(format!("image '{}' dimensions {width}x{height} exceed limits", item.path)));
    }
    Ok(())
}

fn validate_font(archive: &mut ZipArchive<File>, item: &ManifestItem, signatures: &[&[u8]]) -> Result<(), ValidationError> {
    let bytes = read_entry(archive, &item.path, MAX_RESOURCE_BYTES)?;
    if !signatures.iter().any(|signature| bytes.starts_with(signature)) {
        return Err(epub(format!("font '{}' bytes do not match manifest media type '{}'", item.path, item.media_type)));
    }
    Ok(())
}

fn validate_content_xml(bytes: &[u8], path: &str) -> Result<(), ValidationError> {
    let mut reader = Reader::from_reader(bytes);
    reader.trim_text(false);
    let mut buffer = Vec::new();
    loop {
        match reader.read_event_into(&mut buffer).map_err(|error| epub(format!("XML resource '{path}' is invalid: {error}")))? {
            Event::Start(element) | Event::Empty(element) => {
                let name = local_name(element.name().as_ref()).to_ascii_lowercase();
                if matches!(name.as_slice(), b"script" | b"form" | b"audio" | b"video" | b"iframe" | b"object" | b"embed") {
                    return Err(epub(format!("XML resource '{path}' contains prohibited element '{}'", String::from_utf8_lossy(&name))));
                }
                for attribute in element.attributes().with_checks(false) {
                    let attribute = attribute.map_err(|error| epub(format!("XML resource '{path}' has an invalid attribute: {error}")))?;
                    let key = local_name(attribute.key.as_ref()).to_ascii_lowercase();
                    if key.starts_with(b"on") {
                        return Err(epub(format!("XML resource '{path}' contains an event-handler attribute")));
                    }
                    let value = attribute.unescape_value().map_err(epub)?.into_owned().trim().to_ascii_lowercase();
                    if (matches!(key.as_slice(), b"src" | b"href" | b"poster" | b"data") || key == b"style") && ["data:", "file:", "http:", "https:", "javascript:", "//"].iter().any(|prefix| value.contains(prefix)) {
                        return Err(epub(format!("XML resource '{path}' contains a remote, embedded, or executable reference")));
                    }
                }
            }
            Event::DocType(_) => return Err(epub(format!("XML resource '{path}' contains a document type"))),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(())
}

fn validate_pdf(path: &Path) -> Result<(), ValidationError> {
    validate_pdf_eof(path)?;
    let document = Document::load(path).map_err(pdf)?;
    if document.is_encrypted() {
        return Err(pdf("encrypted PDFs are not accepted"));
    }
    if document.objects.len() > MAX_PDF_OBJECTS {
        return Err(pdf("PDF object count exceeds the supported limit"));
    }
    let pages = document.get_pages();
    if pages.is_empty() || pages.len() > MAX_PDF_PAGES {
        return Err(pdf(format!("PDF page count {} is outside the supported range", pages.len())));
    }
    for object in document.objects.values() {
        inspect_pdf_object(object, 0)?;
    }
    inspect_pdf_object(&Object::Dictionary(document.trailer.clone()), 0)?;
    Ok(())
}

fn validate_pdf_eof(path: &Path) -> Result<(), ValidationError> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let tail_len = len.min(4096) as usize;
    file.seek(SeekFrom::End(-(tail_len as i64)))?;
    let mut tail = vec![0_u8; tail_len];
    file.read_exact(&mut tail)?;
    let Some(position) = tail.windows(5).rposition(|window| window == b"%%EOF") else {
        return Err(pdf("missing end-of-file marker"));
    };
    if tail[position + 5..].iter().any(|byte| !byte.is_ascii_whitespace()) {
        return Err(pdf("non-whitespace data follows the final end-of-file marker"));
    }
    Ok(())
}

fn inspect_pdf_object(object: &Object, depth: usize) -> Result<(), ValidationError> {
    if depth > 128 {
        return Err(pdf("PDF object nesting exceeds the supported limit"));
    }
    match object {
        Object::Array(items) => {
            for item in items {
                inspect_pdf_object(item, depth + 1)?;
            }
        }
        Object::Dictionary(dictionary) => inspect_pdf_dictionary(dictionary, depth + 1)?,
        Object::Stream(stream) => inspect_pdf_dictionary(&stream.dict, depth + 1)?,
        _ => {}
    }
    Ok(())
}

fn inspect_pdf_dictionary(dictionary: &Dictionary, depth: usize) -> Result<(), ValidationError> {
    const FORBIDDEN_KEYS: &[&[u8]] = &[
        b"3D",
        b"AA",
        b"AcroForm",
        b"AF",
        b"Collection",
        b"EF",
        b"EmbeddedFiles",
        b"FS",
        b"Filespec",
        b"JavaScript",
        b"JS",
        b"Launch",
        b"MediaClip",
        b"MediaPlayParams",
        b"Movie",
        b"OpenAction",
        b"Rendition",
        b"RichMedia",
        b"RichMediaContent",
        b"RichMediaSettings",
        b"Sound",
        b"URI",
        b"XFA",
    ];
    const FORBIDDEN_NAMES: &[&[u8]] = &[b"3D", b"EmbeddedFile", b"FileAttachment", b"Filespec", b"JavaScript", b"Launch", b"MediaClip", b"Movie", b"Rendition", b"RichMedia", b"Screen", b"Sound", b"Video"];
    for (key, value) in dictionary.iter() {
        if FORBIDDEN_KEYS.contains(&key.as_slice()) {
            return Err(pdf(format!("PDF contains prohibited dictionary key /{}", String::from_utf8_lossy(key))));
        }
        if matches!(key.as_slice(), b"Type" | b"Subtype") {
            if let Object::Name(name) = value {
                if FORBIDDEN_NAMES.contains(&name.as_slice()) {
                    return Err(pdf(format!("PDF contains prohibited object type /{}", String::from_utf8_lossy(name))));
                }
            }
        }
        inspect_pdf_object(value, depth + 1)?;
    }
    Ok(())
}

fn read_entry(archive: &mut ZipArchive<File>, path: &str, limit: u64) -> Result<Vec<u8>, ValidationError> {
    let entry = archive.by_name(path).map_err(|error| epub(format!("cannot read '{path}': {error}")))?;
    if entry.size() > limit {
        return Err(epub(format!("resource '{path}' exceeds its type-specific size limit")));
    }
    let mut bytes = Vec::with_capacity(entry.size().min(1024 * 1024) as usize);
    entry.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(epub(format!("resource '{path}' expands beyond its type-specific size limit")));
    }
    Ok(bytes)
}

fn required_attr(element: &BytesStart<'_>, name: &[u8]) -> Result<String, ValidationError> {
    optional_attr(element, name)?.ok_or_else(|| epub(format!("element '{}' is missing attribute '{}'", String::from_utf8_lossy(local_name(element.name().as_ref())), String::from_utf8_lossy(name))))
}

fn optional_attr(element: &BytesStart<'_>, name: &[u8]) -> Result<Option<String>, ValidationError> {
    let mut found = None;
    for attribute in element.attributes().with_checks(false) {
        let attribute = attribute.map_err(epub)?;
        if local_name(attribute.key.as_ref()) == name {
            if found.is_some() {
                return Err(epub(format!("duplicate attribute '{}'", String::from_utf8_lossy(name))));
            }
            found = Some(attribute.unescape_value().map_err(epub)?.into_owned());
        }
    }
    Ok(found)
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|byte| *byte == b':').next().unwrap_or(name)
}

fn canonical_archive_path(raw: &str) -> Result<String, String> {
    if raw.is_empty() || raw.starts_with('/') || raw.contains('\\') || raw.contains('\0') || raw.contains('%') {
        return Err(format!("unsafe or non-canonical archive path '{raw}'"));
    }
    let mut parts = Vec::new();
    for part in raw.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(format!("unsafe or non-canonical archive path '{raw}'"));
        }
        parts.push(part);
    }
    Ok(parts.join("/"))
}

fn resolve_manifest_path(opf_path: &str, raw_href: &str) -> Result<String, String> {
    if raw_href.is_empty() || raw_href.contains('#') || raw_href.contains('?') || raw_href.contains('\\') || raw_href.contains('\0') || raw_href.starts_with('/') || raw_href.starts_with("//") {
        return Err(format!("unsafe or unsupported manifest href '{raw_href}'"));
    }
    let decoded = percent_decode_str(raw_href).decode_utf8().map_err(|_| format!("manifest href '{raw_href}' is not UTF-8"))?;
    let decoded = decoded.as_ref();
    if decoded.split('/').next().is_some_and(|part| part.contains(':')) {
        return Err(format!("remote manifest href '{raw_href}' is not accepted"));
    }
    let mut parts: Vec<&str> = opf_path.split('/').collect();
    parts.pop();
    for part in decoded.split('/') {
        match part {
            "" | "." => return Err(format!("non-canonical manifest href '{raw_href}'")),
            ".." => {
                if parts.pop().is_none() {
                    return Err(format!("manifest href '{raw_href}' escapes the EPUB root"));
                }
            }
            _ => parts.push(part),
        }
    }
    canonical_archive_path(&parts.join("/"))
}

fn epub(error: impl std::fmt::Display) -> ValidationError {
    ValidationError::InvalidEpub(error.to_string())
}

fn pdf(error: impl std::fmt::Display) -> ValidationError {
    ValidationError::InvalidPdf(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageOutputFormat};
    use lopdf::{dictionary, EncryptionState, EncryptionVersion, Object, Permissions, Stream};
    use std::io::Write;
    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        DynamicImage::new_rgb8(1, 1).write_to(&mut Cursor::new(&mut bytes), ImageOutputFormat::Png).unwrap();
        bytes
    }

    fn epub_file(extra_manifest: &str, extra_files: &[(&str, &[u8])]) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut writer = ZipWriter::new(file.reopen().unwrap());
        writer.start_file("mimetype", SimpleFileOptions::default().compression_method(CompressionMethod::Stored)).unwrap();
        writer.write_all(b"application/epub+zip").unwrap();
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer.start_file("META-INF/container.xml", options).unwrap();
        writer
            .write_all(br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#)
            .unwrap();
        writer.start_file("OEBPS/content.opf", options).unwrap();
        writer
            .write_all(
                format!(
                    r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>{extra_manifest}</manifest><spine><itemref idref="chapter"/></spine></package>"#
                )
                .as_bytes(),
            )
            .unwrap();
        writer.start_file("OEBPS/chapter.xhtml", options).unwrap();
        writer.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Book</p></body></html>"#).unwrap();
        for (path, bytes) in extra_files {
            writer.start_file(*path, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
        file
    }

    #[test]
    fn accepts_valid_epub_and_declared_image() {
        let image = png();
        let file = epub_file(r#"<item id="cover" href="cover.png" media-type="image/png" properties="cover-image"/>"#, &[("OEBPS/cover.png", &image)]);
        assert_eq!(validate_path(file.path()).unwrap().format, BookFormat::Epub);
    }

    #[test]
    fn rejects_undeclared_archive_image() {
        let image = png();
        let file = epub_file("", &[("OEBPS/hidden.png", &image)]);
        assert!(validate_path(file.path()).unwrap_err().to_string().contains("not declared"));
    }

    #[test]
    fn rejects_epub_resource_encryption_and_encrypted_zip_entries() {
        let encryption = epub_file("", &[("META-INF/encryption.xml", b"<encryption/>" as &[u8])]);
        assert!(validate_path(encryption.path()).unwrap_err().to_string().contains("encrypted EPUB"));

        let encrypted_zip = epub_file("", &[]);
        let mut bytes = std::fs::read(encrypted_zip.path()).unwrap();
        for index in 0..bytes.len().saturating_sub(10) {
            let flag_offset = if bytes[index..].starts_with(b"PK\x03\x04") {
                Some(index + 6)
            } else if bytes[index..].starts_with(b"PK\x01\x02") {
                Some(index + 8)
            } else {
                None
            };
            if let Some(flag_offset) = flag_offset {
                bytes[flag_offset] |= 1;
            }
        }
        std::fs::write(encrypted_zip.path(), bytes).unwrap();
        assert!(validate_path(encrypted_zip.path()).unwrap_err().to_string().contains("encrypted ZIP entry"));
    }

    #[test]
    fn rejects_declared_video_and_image_type_lies() {
        let video = epub_file(r#"<item id="movie" href="movie.mp4" media-type="video/mp4"/>"#, &[("OEBPS/movie.mp4", b"not a movie")]);
        assert!(validate_path(video.path()).unwrap_err().to_string().contains("video/mp4"));

        let image = epub_file(r#"<item id="cover" href="cover.png" media-type="image/png"/>"#, &[("OEBPS/cover.png", b"not a png")]);
        assert!(validate_path(image.path()).unwrap_err().to_string().contains("image"));
    }

    #[test]
    fn rejects_raw_images_and_videos_even_when_named_as_books() {
        let directory = tempdir().unwrap();
        for (name, bytes) in [("image.epub", png()), ("video.pdf", b"\0\0\0\x18ftypmp42video".to_vec())] {
            let path = directory.path().join(name);
            std::fs::write(&path, bytes).unwrap();
            assert!(matches!(validate_path(path), Err(ValidationError::UnsupportedFormat)));
        }
    }

    fn pdf_file(prohibited_type: Option<&'static str>, encrypted: bool) -> tempfile::NamedTempFile {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let contents_id = document.add_object(Stream::new(dictionary! {}, Vec::new()));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()],
            "Resources" => dictionary! {},
            "Contents" => contents_id,
        });
        document.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }));
        let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);
        document.trailer.set("ID", Object::Array(vec![Object::string_literal(vec![1_u8; 16]), Object::string_literal(vec![2_u8; 16])]));
        if let Some(prohibited_type) = prohibited_type {
            document.add_object(Stream::new(dictionary! { "Type" => prohibited_type }, b"payload".to_vec()));
        }
        if encrypted {
            let state = EncryptionState::try_from(EncryptionVersion::V2 { document: &document, owner_password: "owner-password", user_password: "user-password", key_length: 128, permissions: Permissions::empty() }).unwrap();
            document.encrypt(&state).unwrap();
        }
        let file = tempfile::NamedTempFile::new().unwrap();
        document.save(file.path()).unwrap();
        file
    }

    #[test]
    fn accepts_structural_pdf_but_rejects_embedded_files_and_media() {
        assert_eq!(validate_path(pdf_file(None, false).path()).unwrap().format, BookFormat::Pdf);
        assert!(validate_path(pdf_file(Some("EmbeddedFile"), false).path()).unwrap_err().to_string().contains("EmbeddedFile"));
        assert!(validate_path(pdf_file(Some("Movie"), false).path()).unwrap_err().to_string().contains("Movie"));
    }

    #[test]
    fn rejects_encrypted_pdfs() {
        assert!(validate_path(pdf_file(None, true).path()).is_err());
    }
}
