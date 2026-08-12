use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, Cursor, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine as _;
use epub_provider::{EpubCreator, EpubIdentifier, EpubMetadata, EpubProvider, IdentifierScheme, MediaOverlayCue};
use html::resources::{FileSystemProvider, ResourceProvider, TocEntry};
use regex::{Captures, Regex};
use serde::{Deserialize, Serialize};

const WEBPUB_MANIFEST: &str = "manifest.json";
const READIUM_CONTEXT: &str = "https://readium.org/webpub-manifest/context.jsonld";
const MAX_WEBPUB_ENTRIES: usize = 20_000;
const MAX_WEBPUB_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
struct WebPubLink {
    href: String,
    #[serde(rename = "type")]
    media_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

#[derive(Deserialize)]
struct WebPubManifest {
    metadata: serde_json::Value,
    #[serde(rename = "readingOrder")]
    reading_order: Vec<WebPubLink>,
    #[serde(default)]
    resources: Vec<WebPubLink>,
}

/// A container-neutral publication ready for the native HTML renderer.
///
/// Non-EPUB adapters expose stable virtual documents and resources without
/// creating or persisting an intermediate EPUB archive.
pub struct PublicationSource {
    pub provider: Arc<dyn ResourceProvider>,
    pub document_uris: Vec<String>,
    pub start_index: usize,
    pub metadata: EpubMetadata,
    pub media_overlay_cues: Vec<MediaOverlayCue>,
    pub layout: PublicationLayout,
}

trait PublicationReadSeek: Read + io::Seek + Send {}
impl<T: Read + io::Seek + Send> PublicationReadSeek for T {}

type PublicationArchive = zip::ZipArchive<Box<dyn PublicationReadSeek>>;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PublicationLayout {
    #[default]
    Reflowable,
    PagedImages {
        page_count: usize,
        reading_direction: ReadingDirection,
        cover_resource: Option<String>,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReadingDirection {
    #[default]
    LeftToRight,
    RightToLeft,
}

impl PublicationSource {
    /// Opens a publication from a seekable source. ZIP-based formats retain the
    /// source and request entries lazily; other formats materialize their bytes.
    pub fn from_reader<R>(path: &Path, mut reader: R) -> io::Result<Self>
    where
        R: Read + io::Seek + Send + 'static,
    {
        let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_ascii_lowercase();
        let publication = match extension.as_str() {
            "epub" => return Self::from_epub_reader(reader),
            "cbz" => Self::cbz_reader(path, reader),
            "webpub" => Self::webpub_reader(reader),
            _ => {
                let mut bytes = Vec::new();
                reader.read_to_end(&mut bytes)?;
                Self::open(path, bytes)
            }
        };
        publication.map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    /// Opens any supported publication while preserving its source format.
    pub fn open(path: &Path, bytes: Vec<u8>) -> Result<Self, String> {
        let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_ascii_lowercase();
        match extension.as_str() {
            "epub" => Self::epub(bytes),
            "html" | "htm" | "xhtml" => Self::html(path),
            "webpub" => Self::webpub(bytes),
            "fb2" => Self::fb2(path, &bytes),
            "mobi" | "azw" | "azw3" => Self::mobi(path, &bytes),
            "cbz" => Self::cbz(path, bytes),
            _ => Err(format!("the native reader has no adapter for .{extension}")),
        }
    }

    /// Opens a source from disk. Directories are treated as loose HTML
    /// publications; files are dispatched to their format adapter.
    pub fn from_path(path: PathBuf) -> io::Result<Self> {
        if path.is_dir() {
            let provider: Arc<dyn ResourceProvider> = Arc::new(FileSystemProvider::new());
            let root = path.to_string_lossy().into_owned();
            let document_uris = provider.list_html_candidates(&root)?;
            let start_index = document_uris.iter().position(|uri| uri.ends_with("index.html")).unwrap_or(0);
            return Self::try_new(provider, document_uris, start_index);
        }
        let bytes = std::fs::read(&path)?;
        Self::open(&path, bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn from_epub_bytes(bytes: Vec<u8>) -> io::Result<Self> {
        Self::epub(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn from_epub_reader<R>(reader: R) -> io::Result<Self>
    where
        R: Read + io::Seek + Send + 'static,
    {
        let provider = EpubProvider::try_from_reader(reader)?;
        Self::from_epub_provider(provider).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn try_new(provider: Arc<dyn ResourceProvider>, document_uris: Vec<String>, start_index: usize) -> io::Result<Self> {
        if document_uris.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "publication contains no HTML documents"));
        }
        if start_index >= document_uris.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "publication start index is outside its document list"));
        }
        Ok(Self { provider, document_uris, start_index, metadata: EpubMetadata::default(), media_overlay_cues: Vec::new(), layout: PublicationLayout::Reflowable })
    }

    pub fn provider(&self) -> Arc<dyn ResourceProvider> {
        self.provider.clone()
    }

    pub fn document_uris(&self) -> &[String] {
        &self.document_uris
    }

    pub fn start_index(&self) -> usize {
        self.start_index
    }

    pub fn into_parts(self) -> (Arc<dyn ResourceProvider>, Vec<String>, usize) {
        (self.provider, self.document_uris, self.start_index)
    }

    fn epub(bytes: Vec<u8>) -> Result<Self, String> {
        let provider = EpubProvider::try_from_reader(Cursor::new(bytes)).map_err(|error| error.to_string())?;
        Self::from_epub_provider(provider)
    }

    fn from_epub_provider(provider: EpubProvider) -> Result<Self, String> {
        let metadata = provider.metadata().map_err(|error| error.to_string())?;
        let media_overlay_cues = provider.media_overlay_cues().map_err(|error| error.to_string())?;
        let document_uris = provider.list_html_candidates("").map_err(|error| error.to_string())?;
        if document_uris.is_empty() {
            return Err("EPUB contains no readable documents".into());
        }
        Ok(Self { provider: Arc::new(provider), document_uris, start_index: 0, metadata, media_overlay_cues, layout: PublicationLayout::Reflowable })
    }

    fn html(path: &Path) -> Result<Self, String> {
        if !path.is_file() {
            return Err(format!("HTML source does not exist: {}", path.display()));
        }
        let provider = SandboxedHtmlProvider::new(path)?;
        let uri = provider.document.to_string_lossy().into_owned();
        let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
        let metadata = html_metadata(path, &source);
        Ok(Self { provider: Arc::new(provider), document_uris: vec![uri], start_index: 0, metadata, media_overlay_cues: Vec::new(), layout: PublicationLayout::Reflowable })
    }

    fn webpub(bytes: Vec<u8>) -> Result<Self, String> {
        Self::webpub_reader(Cursor::new(bytes))
    }

    fn webpub_reader<R>(reader: R) -> Result<Self, String>
    where
        R: Read + io::Seek + Send + 'static,
    {
        let provider = WebPubProvider::from_reader(reader)?;
        let document_uris = provider.reading_order.clone();
        let metadata = webpub_metadata(&provider.metadata);
        Ok(Self { provider: Arc::new(provider), document_uris, start_index: 0, metadata, media_overlay_cues: Vec::new(), layout: PublicationLayout::Reflowable })
    }

    fn fb2(path: &Path, bytes: &[u8]) -> Result<Self, String> {
        let xml = std::str::from_utf8(bytes).map_err(|error| format!("FB2 is not UTF-8: {error}"))?;
        let document = roxmltree::Document::parse(xml).map_err(|error| format!("invalid FB2 XML: {error}"))?;
        if document.root_element().tag_name().name() != "FictionBook" {
            return Err("FB2 document has no FictionBook root".into());
        }

        let mut resources = HashMap::new();
        for binary in document.descendants().filter(|node| node.is_element() && node.tag_name().name() == "binary") {
            let Some(id) = binary.attribute("id") else { continue };
            let encoded = binary.text().unwrap_or_default().chars().filter(|ch| !ch.is_whitespace()).collect::<String>();
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                resources.insert(format!("fb2/resources/{id}"), decoded);
            }
        }

        let mut body = String::new();
        for node in document.root_element().children().filter(|node| node.is_element() && node.tag_name().name() == "body") {
            render_fb2_node(node, &mut body);
        }
        let html = format!(
            "<!doctype html><html><head><meta charset=\"utf-8\"><style>body{{max-width:42em;margin:auto}}.fb2-title{{text-align:center}}.empty-line{{height:1em}}img{{max-width:100%;height:auto}}</style></head><body>{body}</body></html>"
        );
        let uri = "fb2/book.xhtml".to_owned();
        resources.insert(uri.clone(), html.into_bytes());

        let title = document
            .descendants()
            .find(|node| node.is_element() && node.tag_name().name() == "book-title")
            .and_then(|node| node.text())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| path.file_stem().and_then(|s| s.to_str()).unwrap_or("Untitled"))
            .to_owned();
        let creators = document
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "author")
            .take(8)
            .filter_map(|author| {
                let name = ["first-name", "middle-name", "last-name"]
                    .into_iter()
                    .filter_map(|part| author.children().find(|node| node.is_element() && node.tag_name().name() == part).and_then(|node| node.text()))
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                (!name.is_empty()).then_some(EpubCreator::author(name))
            })
            .collect();
        let language = document.descendants().find(|node| node.is_element() && node.tag_name().name() == "lang").and_then(|node| node.text()).unwrap_or_default().trim().to_owned();
        let metadata = EpubMetadata { title, creators, language, ..Default::default() };
        let toc = fb2_toc(&document);
        let provider = VirtualProvider::new(resources, toc);
        Ok(Self { provider: Arc::new(provider), document_uris: vec![uri], start_index: 0, metadata, media_overlay_cues: Vec::new(), layout: PublicationLayout::Reflowable })
    }

    fn mobi(path: &Path, bytes: &[u8]) -> Result<Self, String> {
        let book = mobi::Mobi::new(bytes.to_vec()).map_err(|error| error.to_string())?;
        if !matches!(book.encryption(), mobi::headers::Encryption::No) {
            return Err("DRM-protected MOBI publications are not supported".into());
        }
        let mut resources = HashMap::new();
        for (index, record) in book.image_records().iter().enumerate() {
            resources.insert(format!("mobi/resources/{}", index + 1), record.content.to_vec());
        }

        let source = mobi_content_as_string(&book, bytes)?;
        let image_re = Regex::new(r#"(?i)<img([^>]*?)\srecindex\s*=\s*[\"']?([0-9a-v]+)[\"']?([^>]*)>"#).unwrap();
        let source = image_re.replace_all(&source, |captures: &Captures<'_>| {
            let index = usize::from_str_radix(&captures[2], 32).unwrap_or(0);
            format!("<img{} src=\"../resources/{index}\"{}>", &captures[1], &captures[3])
        });
        let pagebreak = Regex::new(r"(?i)<mbp:pagebreak\s*/?>").unwrap();
        let mut document_uris = Vec::new();
        for (index, section) in pagebreak.split(&source).filter(|section| !section.trim().is_empty()).enumerate() {
            let uri = format!("mobi/sections/{index}.html");
            let html = format!("<!doctype html><html><head><meta charset=\"utf-8\"></head><body>{section}</body></html>");
            resources.insert(uri.clone(), html.into_bytes());
            document_uris.push(uri);
        }
        if document_uris.is_empty() {
            return Err("MOBI contains no readable text".into());
        }
        let mut metadata = EpubMetadata {
            title: if book.title().trim().is_empty() { fallback_title(path) } else { book.title() },
            publisher: book.publisher().unwrap_or_default(),
            description: book.description().unwrap_or_default(),
            date: book.publish_date().unwrap_or_default(),
            ..Default::default()
        };
        if let Some(author) = book.author().filter(|author| !author.trim().is_empty()) {
            metadata.creators.push(EpubCreator::author(author));
        }
        Ok(Self { provider: Arc::new(VirtualProvider::new(resources, Vec::new())), document_uris, start_index: 0, metadata, media_overlay_cues: Vec::new(), layout: PublicationLayout::Reflowable })
    }

    fn cbz(path: &Path, bytes: Vec<u8>) -> Result<Self, String> {
        Self::cbz_reader(path, Cursor::new(bytes))
    }

    fn cbz_reader<R>(path: &Path, reader: R) -> Result<Self, String>
    where
        R: Read + io::Seek + Send + 'static,
    {
        let mut archive = zip::ZipArchive::new(Box::new(reader) as Box<dyn PublicationReadSeek>).map_err(|error| error.to_string())?;
        let comic_info = read_comic_info(&mut archive);
        let mut images = Vec::new();
        for index in 0..archive.len() {
            let entry = archive.by_index_raw(index).map_err(|error| error.to_string())?;
            let name = entry.name().to_owned();
            if !entry.is_dir() && is_image_name(&name) {
                images.push(name);
            }
        }
        images.sort_by_key(|left| natural_key(left));
        if images.is_empty() {
            return Err("CBZ contains no supported images".into());
        }
        let mut documents = HashMap::new();
        let mut document_uris = Vec::with_capacity(images.len());
        let mut image_uris = HashMap::new();
        let mut toc = Vec::with_capacity(images.len());
        let cover_index = comic_info.as_ref().and_then(|info| info.cover_index).filter(|index| *index < images.len()).unwrap_or(0);
        for (index, entry) in images.into_iter().enumerate() {
            let image_uri = format!("cbz/images/{index}");
            let document_uri = format!("cbz/pages/{index}.html");
            image_uris.insert(image_uri.clone(), entry);
            documents.insert(document_uri.clone(), format!("<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><style>html,body{{margin:0;padding:0;width:100%;height:100%}}body{{display:flex;align-items:center;justify-content:center;overflow:hidden}}img{{display:block;max-width:100%;max-height:100vh;width:auto;height:auto;object-fit:contain}}</style></head><body><img src=\"../images/{index}\" alt=\"Page {}\"></body></html>", index + 1).into_bytes());
            toc.push(TocEntry { title: format!("Page {}", index + 1), link: document_uri.clone(), children: Vec::new() });
            document_uris.push(document_uri);
        }
        let provider = CbzProvider { archive: Mutex::new(archive), documents, image_uris, toc };
        let metadata = comic_info.as_ref().map(|info| info.metadata(path)).unwrap_or_else(|| EpubMetadata { title: fallback_title(path), ..Default::default() });
        let reading_direction = comic_info.as_ref().map(|info| info.reading_direction).unwrap_or_default();
        let page_count = document_uris.len();
        Ok(Self {
            provider: Arc::new(provider),
            document_uris,
            start_index: 0,
            metadata,
            media_overlay_cues: Vec::new(),
            layout: PublicationLayout::PagedImages { page_count, reading_direction, cover_resource: Some(format!("cbz/images/{cover_index}")) },
        })
    }
}

fn mobi_content_as_string(book: &mobi::Mobi, source_bytes: &[u8]) -> Result<String, String> {
    use mobi::headers::{Compression, TextEncoding};
    const MAX_MOBI_TEXT_BYTES: usize = 512 * 1024 * 1024;

    // The PalmDOC header owns the number of text records. The optional MOBI
    // first/last-content fields are commonly 0xffff in older files; treating
    // that sentinel as an array index is the panic in mobi 0.8.0.
    let records = book.raw_records();
    let all_records = records.records();
    let (start, end) = mobi_text_record_bounds(book.metadata.mobi.first_content_record, book.metadata.palmdoc.record_count, all_records.len())?;
    let expected_len = book.metadata.palmdoc.text_length as usize;
    if expected_len > MAX_MOBI_TEXT_BYTES {
        return Err(format!("MOBI declares an excessive uncompressed text size of {expected_len} bytes"));
    }
    let mut decoded = Vec::with_capacity(expected_len);
    let extra_data_flags = mobi_extra_record_data_flags(book, source_bytes)?;

    match book.compression() {
        Compression::No => {
            for record in &all_records[start..end] {
                let content = trim_mobi_trailing_data(record.content, extra_data_flags)?;
                let remaining = expected_len.saturating_sub(decoded.len());
                decoded.extend_from_slice(&content[..content.len().min(remaining)]);
                if decoded.len() == expected_len {
                    break;
                }
            }
        }
        Compression::PalmDoc => {
            for record in &all_records[start..end] {
                let content = trim_mobi_trailing_data(record.content, extra_data_flags)?;
                palmdoc_decompress_into(content, &mut decoded, expected_len)?;
                if decoded.len() == expected_len {
                    break;
                }
            }
        }
        Compression::Huff => {
            // mobi keeps its HUFF/CDIC implementation private. Preserve support
            // for it, but turn parser panics into a normal unsupported-file
            // error at our publication boundary.
            let declared = book.readable_records_range();
            let huff_start = book.metadata.mobi.first_huff_record as usize;
            let huff_end = huff_start.checked_add(book.metadata.mobi.huff_record_count as usize);
            if declared.start >= declared.end || declared.end > all_records.len() || !huff_end.is_some_and(|end| end <= all_records.len()) {
                return Err("MOBI HUFF/CDIC record bounds are invalid".to_owned());
            }
            return std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| book.content_as_string())).map_err(|_| "MOBI HUFF/CDIC text record bounds are invalid".to_owned())?.map_err(|error| error.to_string());
        }
    }

    match book.text_encoding() {
        TextEncoding::CP1252 => Ok(encoding_rs::WINDOWS_1252.decode_without_bom_handling(&decoded).0.into_owned()),
        TextEncoding::UTF8 => String::from_utf8(decoded).map_err(|error| error.to_string()),
        TextEncoding::Unknown(_) => Ok(String::from_utf8_lossy(&decoded).into_owned()),
    }
}

fn mobi_extra_record_data_flags(book: &mobi::Mobi, source_bytes: &[u8]) -> Result<u32, String> {
    const PALMDOC_HEADER_LEN: usize = 16;
    const EXTRA_DATA_FLAGS_OFFSET: usize = 0xe0;
    let Some(record_zero) = book.metadata.records.records.first() else {
        return Err("MOBI contains no header record".to_owned());
    };
    if book.metadata.mobi.header_length < (EXTRA_DATA_FLAGS_OFFSET + 4) as u32 {
        return Ok(0);
    }
    let offset = usize::try_from(record_zero.offset).ok().and_then(|offset| offset.checked_add(PALMDOC_HEADER_LEN + EXTRA_DATA_FLAGS_OFFSET)).ok_or_else(|| "MOBI extra-data flags offset overflows".to_owned())?;
    let bytes = source_bytes.get(offset..offset + 4).ok_or_else(|| "MOBI extra-data flags are truncated".to_owned())?;
    Ok(u32::from_be_bytes(bytes.try_into().unwrap()))
}

fn trim_mobi_trailing_data(mut data: &[u8], flags: u32) -> Result<&[u8], String> {
    let mut trailing_entries = flags >> 1;
    while trailing_entries != 0 {
        if trailing_entries & 1 != 0 {
            let size = mobi_trailing_entry_size(data)?;
            data = data.get(..data.len() - size).ok_or_else(|| "MOBI trailing-data entry exceeds its text record".to_owned())?;
        }
        trailing_entries >>= 1;
    }
    if flags & 1 != 0 {
        let last = *data.last().ok_or_else(|| "MOBI multibyte trailer is missing".to_owned())?;
        let size = usize::from(last & 0x03) + 1;
        if size > data.len() {
            return Err("MOBI multibyte trailer exceeds its text record".to_owned());
        }
        data = &data[..data.len() - size];
    }
    Ok(data)
}

fn mobi_trailing_entry_size(data: &[u8]) -> Result<usize, String> {
    let mut result = 0usize;
    let mut bit_position = 0u32;
    for byte in data.iter().rev().take(4) {
        result |= usize::from(byte & 0x7f) << bit_position;
        if byte & 0x80 != 0 {
            return (result > 0 && result <= data.len()).then_some(result).ok_or_else(|| "invalid MOBI trailing-data entry size".to_owned());
        }
        bit_position += 7;
    }
    Err("unterminated MOBI trailing-data entry size".to_owned())
}

fn mobi_text_record_bounds(first_content_record: u16, record_count: u16, total_records: usize) -> Result<(usize, usize), String> {
    let record_count = usize::from(record_count);
    if record_count == 0 {
        return Err("MOBI contains no text records".to_owned());
    }
    let declared_start = usize::from(first_content_record);
    let declared_is_usable = first_content_record != u16::MAX && declared_start > 0 && declared_start.checked_add(record_count).is_some_and(|end| end <= total_records);
    let start = if declared_is_usable { declared_start } else { 1 };
    let end = start.checked_add(record_count).ok_or_else(|| "MOBI text record range overflows".to_owned())?;
    if end > total_records {
        return Err(format!("MOBI declares {record_count} text records but contains only {} data records", total_records.saturating_sub(start)));
    }
    Ok((start, end))
}

fn palmdoc_decompress_into(data: &[u8], output: &mut Vec<u8>, output_limit: usize) -> Result<(), String> {
    let mut position = 0;
    while position < data.len() && output.len() < output_limit {
        let byte = data[position];
        position += 1;
        match byte {
            0 | 0x09..=0x7f => output.push(byte),
            1..=8 => {
                let literal_count = usize::from(byte);
                let end = position.checked_add(literal_count).filter(|end| *end <= data.len()).ok_or_else(|| "truncated PalmDOC literal run".to_owned())?;
                let remaining = output_limit - output.len();
                output.extend_from_slice(&data[position..end][..literal_count.min(remaining)]);
                position = end;
            }
            0x80..=0xbf => {
                let next = *data.get(position).ok_or_else(|| "truncated PalmDOC back-reference".to_owned())?;
                position += 1;
                let pair = u16::from_be_bytes([byte, next]);
                let distance = usize::from((pair >> 3) & 0x07ff);
                let length = usize::from((pair & 0x0007) + 3);
                if distance == 0 || distance > output.len() {
                    return Err("invalid PalmDOC back-reference distance".to_owned());
                }
                for _ in 0..length {
                    if output.len() == output_limit {
                        break;
                    }
                    let source = output.len() - distance;
                    output.push(output[source]);
                }
            }
            _ => {
                if output.len() < output_limit {
                    output.push(b' ');
                }
                if output.len() < output_limit {
                    output.push(byte ^ 0x80);
                }
            }
        }
    }
    Ok(())
}

/// Packages standalone HTML and its reachable local resources as a Readium
/// Web Publication. Source bytes and relative paths are not rewritten.
pub fn bundle_webpub(entry_path: &Path) -> Result<Vec<u8>, String> {
    let entry_path = entry_path.canonicalize().map_err(|error| error.to_string())?;
    let root = entry_path.parent().ok_or_else(|| "HTML entry point has no parent directory".to_owned())?.to_path_buf();
    let entry_name = entry_path.file_name().and_then(|name| name.to_str()).ok_or_else(|| "HTML entry point name is not UTF-8".to_owned())?.to_owned();
    let mut queue = VecDeque::from([entry_path.clone()]);
    let mut visited = HashSet::new();
    let mut files = Vec::new();
    let mut total = 0_u64;

    while let Some(path) = queue.pop_front() {
        let canonical = path.canonicalize().map_err(|error| format!("cannot read referenced HTML resource {}: {error}", path.display()))?;
        if !canonical.starts_with(&root) || !canonical.is_file() {
            continue;
        }
        let relative = path.strip_prefix(&root).map_err(|error| error.to_string())?;
        let archive_name = archive_path(relative)?;
        if archive_name == WEBPUB_MANIFEST {
            return Err(format!("HTML source reserves the archive path {WEBPUB_MANIFEST}"));
        }
        if !visited.insert(archive_name.clone()) {
            continue;
        }
        if visited.len() > MAX_WEBPUB_ENTRIES {
            return Err("Web Publication contains too many referenced files".to_owned());
        }
        let bytes = std::fs::read(&canonical).map_err(|error| error.to_string())?;
        total = total.checked_add(bytes.len() as u64).ok_or_else(|| "Web Publication size overflow".to_owned())?;
        if total > MAX_WEBPUB_BYTES {
            return Err("Web Publication exceeds the one-gigabyte size limit".to_owned());
        }
        if is_text_dependency(&canonical) {
            let source = String::from_utf8_lossy(&bytes);
            for reference in local_references(&source) {
                if let Some(candidate) = resolve_local_dependency(&root, path.parent().unwrap_or(&root), &reference) {
                    queue.push_back(candidate);
                }
            }
        }
        files.push((archive_name, bytes));
    }

    files.sort_by(|left, right| left.0.cmp(&right.0));
    let entry_source = std::fs::read_to_string(&entry_path).map_err(|error| error.to_string())?;
    let extracted = html_metadata(&entry_path, &entry_source);
    let mut metadata = serde_json::Map::from_iter([("@type".to_owned(), serde_json::Value::String(html_publication_type(&entry_source).to_owned())), ("title".to_owned(), serde_json::Value::String(extracted.title.clone()))]);
    if !extracted.creators.is_empty() {
        metadata.insert("author".to_owned(), serde_json::Value::Array(extracted.creators.iter().map(|creator| serde_json::Value::String(creator.name.clone())).collect()));
    }
    for (key, value) in [("publisher", &extracted.publisher), ("language", &extracted.language), ("description", &extracted.description), ("published", &extracted.date)] {
        if !value.is_empty() {
            metadata.insert(key.to_owned(), serde_json::Value::String(value.clone()));
        }
    }
    let reading_order = vec![WebPubLink { href: webpub_href(&entry_name), media_type: media_type_for_path(&entry_name).to_owned(), title: Some(extracted.title) }];
    let resources = files.iter().filter(|(name, _)| name != &entry_name).map(|(name, _)| WebPubLink { href: webpub_href(name), media_type: media_type_for_path(name).to_owned(), title: None }).collect::<Vec<_>>();
    let mut links = vec![serde_json::json!({ "rel": "self", "href": WEBPUB_MANIFEST, "type": "application/webpub+json" })];
    if let Some(href) = html_canonical_url(&entry_source) {
        links.push(serde_json::json!({ "rel": "canonical", "href": href }));
    }
    let manifest = serde_json::to_vec(&serde_json::json!({
        "@context": READIUM_CONTEXT,
        "metadata": metadata,
        "links": links,
        "readingOrder": reading_order,
        "resources": resources,
    }))
    .map_err(|error| error.to_string())?;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    writer.start_file(WEBPUB_MANIFEST, deflated).map_err(|error| error.to_string())?;
    writer.write_all(&manifest).map_err(|error| error.to_string())?;
    for (name, bytes) in files {
        let media_type = media_type_for_path(&name);
        let options = if media_type.starts_with("image/") || media_type.starts_with("audio/") || media_type.starts_with("video/") || media_type.starts_with("font/") {
            zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)
        } else {
            deflated
        };
        writer.start_file(name, options).map_err(|error| error.to_string())?;
        writer.write_all(&bytes).map_err(|error| error.to_string())?;
    }
    writer.finish().map_err(|error| error.to_string()).map(|cursor| cursor.into_inner())
}

fn archive_path(path: &Path) -> Result<String, String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_str().ok_or_else(|| "HTML resource path is not UTF-8".to_owned())?),
            _ => return Err("HTML resource path is not a safe relative path".to_owned()),
        }
    }
    if parts.is_empty() { Err("HTML resource path is empty".to_owned()) } else { Ok(parts.join("/")) }
}

fn is_text_dependency(path: &Path) -> bool {
    matches!(path.extension().and_then(|value| value.to_str()).map(|value| value.to_ascii_lowercase()).as_deref(), Some("html" | "htm" | "xhtml" | "css" | "svg"))
}

fn local_references(source: &str) -> Vec<String> {
    let attributes = Regex::new(r#"(?is)\b(?:src|href|poster|data)\s*=\s*["']([^"']+)["']"#).unwrap();
    let srcsets = Regex::new(r#"(?is)\bsrcset\s*=\s*["']([^"']+)["']"#).unwrap();
    let css_urls = Regex::new(r#"(?is)(?:url\(\s*["']?([^\)"']+)|@import\s+["']([^"']+))"#).unwrap();
    let mut references = attributes.captures_iter(source).filter_map(|capture| capture.get(1).map(|value| value.as_str().to_owned())).collect::<Vec<_>>();
    references.extend(srcsets.captures_iter(source).flat_map(|capture| capture[1].split(',').filter_map(|candidate| candidate.split_whitespace().next().map(str::to_owned)).collect::<Vec<_>>()));
    references.extend(css_urls.captures_iter(source).filter_map(|capture| capture.get(1).or_else(|| capture.get(2)).map(|value| value.as_str().trim().to_owned())));
    references
}

fn resolve_local_dependency(root: &Path, base: &Path, reference: &str) -> Option<PathBuf> {
    let reference = reference.trim();
    if reference.is_empty() || reference.starts_with('#') || reference.starts_with("//") || reference.starts_with("data:") {
        return None;
    }
    let without_suffix = reference.split(['#', '?']).next().unwrap_or(reference);
    if without_suffix.split('/').next().is_some_and(|head| head.contains(':')) {
        return None;
    }
    let decoded = percent_encoding::percent_decode_str(without_suffix).decode_utf8().ok()?;
    let candidate = if decoded.starts_with('/') { root.join(decoded.trim_start_matches('/')) } else { base.join(decoded.as_ref()) };
    let mut normalized = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            component => normalized.push(component.as_os_str()),
        }
    }
    if !normalized.starts_with(root) {
        return None;
    }
    if normalized.is_dir() {
        normalized = normalized.join("index.html");
    }
    normalized.is_file().then_some(normalized)
}

struct WebPubProvider {
    archive: Mutex<PublicationArchive>,
    entries: HashSet<String>,
    reading_order: Vec<String>,
    metadata: serde_json::Value,
}

impl WebPubProvider {
    fn new(bytes: Vec<u8>) -> Result<Self, String> {
        Self::from_reader(Cursor::new(bytes))
    }

    fn from_reader<R>(reader: R) -> Result<Self, String>
    where
        R: Read + io::Seek + Send + 'static,
    {
        let mut archive = zip::ZipArchive::new(Box::new(reader) as Box<dyn PublicationReadSeek>).map_err(|error| error.to_string())?;
        let entries = (0..archive.len()).filter_map(|index| archive.by_index_raw(index).ok().filter(|entry| !entry.is_dir()).map(|entry| entry.name().to_owned())).collect::<HashSet<_>>();
        let manifest: WebPubManifest = {
            let mut entry = archive.by_name(WEBPUB_MANIFEST).map_err(|_| "Web Publication has no manifest.json".to_owned())?;
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
            serde_json::from_slice(&bytes).map_err(|error| format!("invalid Web Publication manifest: {error}"))?
        };
        if manifest.reading_order.is_empty() {
            return Err("Web Publication manifest has no readingOrder".to_owned());
        }
        let reading_order = manifest.reading_order.iter().map(|link| webpub_archive_path(&link.href)).collect::<Result<Vec<_>, _>>()?;
        let resources = manifest.resources.iter().map(|link| webpub_archive_path(&link.href)).collect::<Result<Vec<_>, _>>()?;
        if reading_order.iter().any(|href| !entries.contains(href)) || resources.iter().any(|href| !entries.contains(href)) {
            return Err("Web Publication manifest references a missing resource".to_owned());
        }
        Ok(Self { archive: Mutex::new(archive), entries, reading_order, metadata: manifest.metadata })
    }
}

impl ResourceProvider for WebPubProvider {
    fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
        if !self.entries.contains(uri) {
            return Err(io::Error::new(io::ErrorKind::NotFound, uri.to_owned()));
        }
        let mut archive = self.archive.lock().map_err(|_| io::Error::other("Web Publication archive lock poisoned"))?;
        let mut entry = archive.by_name(uri).map_err(|error| io::Error::new(io::ErrorKind::NotFound, error))?;
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        Ok(bytes)
    }
    fn exists(&self, uri: &str) -> bool {
        self.entries.contains(uri)
    }
    fn resolve(&self, base: &str, href: &str) -> String {
        if href.contains("://") || href.starts_with("data:") || href.starts_with("javascript:") {
            return ".blocked-external-resource".to_owned();
        }
        resolve_virtual(base, href)
    }
    fn list_html_candidates(&self, _root: &str) -> io::Result<Vec<String>> {
        Ok(self.reading_order.clone())
    }
}

struct SandboxedHtmlProvider {
    root: PathBuf,
    document: PathBuf,
}

impl SandboxedHtmlProvider {
    fn new(path: &Path) -> Result<Self, String> {
        let document = path.canonicalize().map_err(|error| error.to_string())?;
        let root = document.parent().ok_or_else(|| "HTML document has no parent directory".to_owned())?.to_path_buf();
        Ok(Self { root, document })
    }

    fn allowed_path(&self, path: &Path) -> Option<PathBuf> {
        let canonical = path.canonicalize().ok()?;
        canonical.starts_with(&self.root).then_some(canonical)
    }
}

impl ResourceProvider for SandboxedHtmlProvider {
    fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
        let path = self.allowed_path(Path::new(uri)).ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "HTML resource escapes its document directory"))?;
        std::fs::read(path)
    }
    fn exists(&self, uri: &str) -> bool {
        self.allowed_path(Path::new(uri)).is_some_and(|path| path.is_file())
    }
    fn resolve(&self, base: &str, href: &str) -> String {
        if href.contains("://") || href.starts_with("data:") || href.starts_with("javascript:") {
            return self.root.join(".blocked-external-resource").to_string_lossy().into_owned();
        }
        let href = href.split(['#', '?']).next().unwrap_or(href);
        let base = Path::new(base);
        let base_dir = if base.as_os_str().to_string_lossy().ends_with('/') { base } else { base.parent().unwrap_or(&self.root) };
        let candidate = if Path::new(href).is_absolute() { PathBuf::from(href) } else { base_dir.join(href) };
        let mut normalized = PathBuf::new();
        for component in candidate.components() {
            match component {
                Component::ParentDir => {
                    normalized.pop();
                }
                Component::CurDir => {}
                component => normalized.push(component.as_os_str()),
            }
        }
        if normalized.starts_with(&self.root) { normalized.to_string_lossy().into_owned() } else { self.root.join(".blocked-resource").to_string_lossy().into_owned() }
    }
    fn list_html_candidates(&self, _root: &str) -> io::Result<Vec<String>> {
        Ok(vec![self.document.to_string_lossy().into_owned()])
    }
}

struct VirtualProvider {
    resources: HashMap<String, Vec<u8>>,
    toc: Vec<TocEntry>,
}

impl VirtualProvider {
    fn new(resources: HashMap<String, Vec<u8>>, toc: Vec<TocEntry>) -> Self {
        Self { resources, toc }
    }
}

impl ResourceProvider for VirtualProvider {
    fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
        self.resources.get(uri).cloned().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, uri.to_owned()))
    }
    fn exists(&self, uri: &str) -> bool {
        self.resources.contains_key(uri)
    }
    fn resolve(&self, base: &str, href: &str) -> String {
        resolve_virtual(base, href)
    }
    fn list_html_candidates(&self, _root: &str) -> io::Result<Vec<String>> {
        let mut docs = self.resources.keys().filter(|uri| uri.ends_with(".html") || uri.ends_with(".xhtml")).cloned().collect::<Vec<_>>();
        docs.sort();
        Ok(docs)
    }
    fn toc(&self) -> io::Result<Option<Vec<TocEntry>>> {
        Ok((!self.toc.is_empty()).then(|| self.toc.clone()))
    }
}

struct CbzProvider {
    archive: Mutex<PublicationArchive>,
    documents: HashMap<String, Vec<u8>>,
    image_uris: HashMap<String, String>,
    toc: Vec<TocEntry>,
}

impl ResourceProvider for CbzProvider {
    fn read_bytes(&self, uri: &str) -> io::Result<Vec<u8>> {
        if let Some(document) = self.documents.get(uri) {
            return Ok(document.clone());
        }
        let entry_name = self.image_uris.get(uri).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, uri.to_owned()))?;
        let mut archive = self.archive.lock().map_err(|_| io::Error::other("CBZ archive lock poisoned"))?;
        let mut entry = archive.by_name(entry_name).map_err(|error| io::Error::new(io::ErrorKind::NotFound, error))?;
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        Ok(bytes)
    }
    fn exists(&self, uri: &str) -> bool {
        self.documents.contains_key(uri) || self.image_uris.contains_key(uri)
    }
    fn resolve(&self, base: &str, href: &str) -> String {
        resolve_virtual(base, href)
    }
    fn list_html_candidates(&self, _root: &str) -> io::Result<Vec<String>> {
        let mut docs = self.documents.keys().cloned().collect::<Vec<_>>();
        docs.sort_by_key(|uri| uri.trim_end_matches(".html").rsplit('/').next().and_then(|value| value.parse::<usize>().ok()).unwrap_or(usize::MAX));
        Ok(docs)
    }
    fn toc(&self) -> io::Result<Option<Vec<TocEntry>>> {
        Ok(Some(self.toc.clone()))
    }
}

fn resolve_virtual(base: &str, href: &str) -> String {
    let href = href.split(['#', '?']).next().unwrap_or(href);
    if href.starts_with('/') {
        return href.trim_start_matches('/').to_owned();
    }
    let mut parts = base.rsplit_once('/').map(|(parent, _)| parent.split('/').collect::<Vec<_>>()).unwrap_or_default();
    for part in href.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

fn render_fb2_node(node: roxmltree::Node<'_, '_>, out: &mut String) {
    if node.is_text() {
        escape_text(node.text().unwrap_or_default(), out);
        return;
    }
    if !node.is_element() {
        return;
    }
    let name = node.tag_name().name();
    if matches!(name, "binary" | "description") {
        return;
    }
    let tag = match name {
        "FictionBook" | "body" => "div",
        "section" => "section",
        "title" => "h1",
        "subtitle" => "h2",
        "p" | "text-author" | "date" => "p",
        "emphasis" => "em",
        "strong" => "strong",
        "strikethrough" => "s",
        "code" => "code",
        "epigraph" | "cite" => "blockquote",
        "poem" | "stanza" => "div",
        "v" => "p",
        "empty-line" => "div",
        "table" => "table",
        "tr" => "tr",
        "td" => "td",
        "th" => "th",
        "a" => "a",
        "image" => "img",
        _ => "span",
    };
    out.push('<');
    out.push_str(tag);
    if name == "title" {
        out.push_str(" class=\"fb2-title\"");
    }
    if name == "empty-line" {
        out.push_str(" class=\"empty-line\"");
    }
    if let Some(id) = node.attribute("id") {
        out.push_str(" id=\"");
        escape_attr(id, out);
        out.push('"');
    }
    if name == "a"
        && let Some(href) = node.attributes().find(|attribute| attribute.name() == "href").map(|attribute| attribute.value())
    {
        out.push_str(" href=\"");
        escape_attr(href, out);
        out.push('"');
    }
    if name == "image"
        && let Some(href) = node.attributes().find(|attribute| attribute.name() == "href").map(|attribute| attribute.value())
    {
        out.push_str(" src=\"resources/");
        escape_attr(href.trim_start_matches('#'), out);
        out.push_str("\" alt=\"\"");
    }
    out.push('>');
    if name != "image" {
        for child in node.children() {
            render_fb2_node(child, out);
        }
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
}

fn fb2_toc(document: &roxmltree::Document<'_>) -> Vec<TocEntry> {
    fn section_entry(section: roxmltree::Node<'_, '_>) -> Option<TocEntry> {
        let id = section.attribute("id")?;
        let title = section.children().find(|child| child.is_element() && child.tag_name().name() == "title").map(compact_node_text).filter(|title| !title.is_empty())?;
        let children = section.children().filter(|child| child.is_element() && child.tag_name().name() == "section").filter_map(section_entry).collect();
        Some(TocEntry { title, link: format!("fb2/book.xhtml#{id}"), children })
    }

    document
        .root_element()
        .children()
        .filter(|node| node.is_element() && node.tag_name().name() == "body")
        .flat_map(|body| body.children())
        .filter(|node| node.is_element() && node.tag_name().name() == "section")
        .filter_map(section_entry)
        .collect()
}

fn compact_node_text(node: roxmltree::Node<'_, '_>) -> String {
    node.descendants().filter(|child| child.is_text()).filter_map(|child| child.text()).flat_map(str::split_whitespace).collect::<Vec<_>>().join(" ")
}

#[derive(Default)]
struct ComicInfo {
    title: Option<String>,
    series: Option<String>,
    number: Option<String>,
    writers: Vec<String>,
    summary: Option<String>,
    publisher: Option<String>,
    language: Option<String>,
    date: Option<String>,
    cover_index: Option<usize>,
    reading_direction: ReadingDirection,
}

impl ComicInfo {
    fn metadata(&self, path: &Path) -> EpubMetadata {
        let title = self.title.clone().or_else(|| match (&self.series, &self.number) {
            (Some(series), Some(number)) => Some(format!("{series} #{number}")),
            (Some(series), None) => Some(series.clone()),
            _ => None,
        });
        EpubMetadata {
            title: title.unwrap_or_else(|| fallback_title(path)),
            creators: self.writers.iter().map(|name| EpubCreator::author(name.clone())).collect(),
            description: self.summary.clone().unwrap_or_default(),
            publisher: self.publisher.clone().unwrap_or_default(),
            language: self.language.clone().unwrap_or_default(),
            date: self.date.clone().unwrap_or_default(),
            ..Default::default()
        }
    }
}

fn read_comic_info(archive: &mut PublicationArchive) -> Option<ComicInfo> {
    let index =
        (0..archive.len()).find(|index| archive.by_index_raw(*index).ok().and_then(|entry| Path::new(entry.name()).file_name().and_then(|name| name.to_str()).map(|name| name.eq_ignore_ascii_case("ComicInfo.xml"))).unwrap_or(false))?;
    let mut entry = archive.by_index(index).ok()?;
    let mut bytes = Vec::new();
    entry.read_to_end(&mut bytes).ok()?;
    let xml = std::str::from_utf8(&bytes).ok()?.trim_start_matches('\u{feff}');
    let document = roxmltree::Document::parse(xml).ok()?;
    let value = |name: &str| document.descendants().find(|node| node.is_element() && node.tag_name().name().eq_ignore_ascii_case(name)).and_then(|node| node.text()).map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned);
    let writers = value("Writer").into_iter().flat_map(|writers| writers.split([',', ';']).map(str::trim).filter(|writer| !writer.is_empty()).map(str::to_owned).collect::<Vec<_>>()).collect();
    let year = value("Year");
    let month = value("Month");
    let day = value("Day");
    let date = year.map(|year| match (month, day) {
        (Some(month), Some(day)) => format!("{year}-{month:0>2}-{day:0>2}"),
        (Some(month), None) => format!("{year}-{month:0>2}"),
        _ => year,
    });
    let cover_index = document
        .descendants()
        .find(|node| node.is_element() && node.tag_name().name().eq_ignore_ascii_case("Page") && node.attribute("Type").is_some_and(|kind| kind.eq_ignore_ascii_case("FrontCover")))
        .and_then(|node| node.attribute("Image"))
        .and_then(|index| index.parse().ok());
    let reading_direction = if value("Manga").is_some_and(|value| value.eq_ignore_ascii_case("YesAndRightToLeft")) { ReadingDirection::RightToLeft } else { ReadingDirection::LeftToRight };
    Some(ComicInfo { title: value("Title"), series: value("Series"), number: value("Number"), writers, summary: value("Summary"), publisher: value("Publisher"), language: value("LanguageISO"), date, cover_index, reading_direction })
}

fn escape_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
}

fn html_metadata(path: &Path, source: &str) -> EpubMetadata {
    let title = html_meta_content(source, &["og:title", "twitter:title"])
        .or_else(|| {
            Regex::new(r"(?is)<title(?:\s[^>]*)?>(.*?)</title>")
                .ok()
                .and_then(|regex| regex.captures(source))
                .and_then(|captures| captures.get(1))
                .map(|title| Regex::new(r"(?s)<[^>]+>").unwrap().replace_all(title.as_str(), " ").split_whitespace().collect::<Vec<_>>().join(" "))
        })
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| fallback_title(path));
    let author = html_meta_content(source, &["author", "article:author"]);
    let publisher = html_meta_content(source, &["publisher", "og:site_name"]).unwrap_or_default();
    let description = html_meta_content(source, &["description", "og:description", "twitter:description"]).unwrap_or_default();
    let language = html_attribute(source, "html", "lang").unwrap_or_default();
    let date = html_meta_content(source, &["article:published_time", "datepublished", "date"]).or_else(|| html_meta_content(source, &["article:modified_time", "datemodified"])).unwrap_or_default();
    let identifiers = html_canonical_url(source).into_iter().map(|value| EpubIdentifier { value, scheme: IdentifierScheme::Other("URL".to_owned()), identifier_types: Vec::new() }).collect();
    EpubMetadata { title, identifiers, creators: author.into_iter().map(EpubCreator::author).collect(), description, publisher, language, date, ..Default::default() }
}

fn webpub_metadata(value: &serde_json::Value) -> EpubMetadata {
    let title = value.get("title").and_then(serde_json::Value::as_str).unwrap_or("Untitled").to_owned();
    let creators = value.get("author").into_iter().flat_map(json_names).map(EpubCreator::author).collect();
    let identifiers = value.get("identifier").into_iter().flat_map(json_names).map(|value| EpubIdentifier { value, scheme: IdentifierScheme::Other("WEBPUB".to_owned()), identifier_types: Vec::new() }).collect();
    EpubMetadata {
        title,
        identifiers,
        creators,
        description: json_text(value.get("description")),
        publisher: value.get("publisher").into_iter().flat_map(json_names).next().unwrap_or_default(),
        language: json_text(value.get("language")),
        date: json_text(value.get("published")).or_else_nonempty(|| json_text(value.get("modified"))),
        ..Default::default()
    }
}

trait NonEmptyString {
    fn or_else_nonempty(self, fallback: impl FnOnce() -> String) -> String;
}

impl NonEmptyString for String {
    fn or_else_nonempty(self, fallback: impl FnOnce() -> String) -> String {
        if self.is_empty() { fallback() } else { self }
    }
}

fn json_text(value: Option<&serde_json::Value>) -> String {
    value.and_then(serde_json::Value::as_str).unwrap_or_default().to_owned()
}

fn json_names(value: &serde_json::Value) -> Box<dyn Iterator<Item = String> + '_> {
    let values = match value {
        serde_json::Value::Array(values) => values.as_slice(),
        value => std::slice::from_ref(value),
    };
    Box::new(values.iter().filter_map(|value| value.as_str().or_else(|| value.get("name").and_then(serde_json::Value::as_str))).map(str::to_owned))
}

fn html_meta_content(source: &str, names: &[&str]) -> Option<String> {
    let tags = Regex::new(r"(?is)<meta\b[^>]*>").ok()?;
    tags.find_iter(source).find_map(|tag| {
        let key = html_tag_attribute(tag.as_str(), "name").or_else(|| html_tag_attribute(tag.as_str(), "property")).or_else(|| html_tag_attribute(tag.as_str(), "itemprop"))?;
        names.iter().any(|name| key.eq_ignore_ascii_case(name)).then(|| html_tag_attribute(tag.as_str(), "content")).flatten()
    })
}

fn html_canonical_url(source: &str) -> Option<String> {
    let tags = Regex::new(r"(?is)<link\b[^>]*>").ok()?;
    tags.find_iter(source).find_map(|tag| {
        let rel = html_tag_attribute(tag.as_str(), "rel")?;
        rel.split_whitespace().any(|value| value.eq_ignore_ascii_case("canonical")).then(|| html_tag_attribute(tag.as_str(), "href")).flatten()
    })
}

fn html_attribute(source: &str, tag: &str, attribute: &str) -> Option<String> {
    let tag = Regex::new(&format!(r"(?is)<{}\b[^>]*>", regex::escape(tag))).ok()?.find(source)?;
    html_tag_attribute(tag.as_str(), attribute)
}

fn html_tag_attribute(tag: &str, attribute: &str) -> Option<String> {
    let expression = Regex::new(&format!(r#"(?is)\b{}\s*=\s*(?:"([^"]*)"|'([^']*)')"#, regex::escape(attribute))).ok()?;
    expression.captures(tag).and_then(|captures| captures.get(1).or_else(|| captures.get(2))).map(|value| value.as_str().trim().to_owned()).filter(|value| !value.is_empty())
}

fn html_publication_type(source: &str) -> &'static str {
    if source.to_ascii_lowercase().contains("<article") || html_meta_content(source, &["og:type"]).is_some_and(|value| value.eq_ignore_ascii_case("article")) { "https://schema.org/Article" } else { "https://schema.org/CreativeWork" }
}

fn media_type_for_path(path: &str) -> &'static str {
    match Path::new(path).extension().and_then(|value| value.to_str()).unwrap_or_default().to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html",
        "xhtml" => "application/xhtml+xml",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "mp4" => "video/mp4",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

fn webpub_href(path: &str) -> String {
    percent_encoding::utf8_percent_encode(path, percent_encoding::NON_ALPHANUMERIC).to_string().replace("%2F", "/").replace("%2E", ".").replace("%2D", "-").replace("%5F", "_").replace("%7E", "~")
}

fn webpub_archive_path(href: &str) -> Result<String, String> {
    let href = href.split(['?', '#']).next().unwrap_or(href);
    let decoded = percent_encoding::percent_decode_str(href).decode_utf8().map_err(|error| error.to_string())?;
    if decoded.starts_with('/') || decoded.split('/').any(|part| matches!(part, "" | "." | "..")) {
        return Err("Web Publication contains a non-canonical resource href".to_owned());
    }
    Ok(decoded.into_owned())
}
fn escape_attr(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}
fn fallback_title(path: &Path) -> String {
    path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("Untitled").to_owned()
}
fn is_image_name(name: &str) -> bool {
    matches!(Path::new(name).extension().and_then(|value| value.to_str()).map(|value| value.to_ascii_lowercase()).as_deref(), Some("jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg"))
}
fn natural_key(name: &str) -> Vec<NaturalPart> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut digits = None;
    for ch in name.to_ascii_lowercase().chars() {
        let is_digit = ch.is_ascii_digit();
        if digits.is_some_and(|value| value != is_digit) {
            parts.push(if digits == Some(true) { NaturalPart::Number(current.parse().unwrap_or(0)) } else { NaturalPart::Text(std::mem::take(&mut current)) });
            current.clear();
        }
        digits = Some(is_digit);
        current.push(ch);
    }
    if !current.is_empty() {
        parts.push(if digits == Some(true) { NaturalPart::Number(current.parse().unwrap_or(0)) } else { NaturalPart::Text(current) });
    }
    parts
}
#[derive(Eq, Ord, PartialEq, PartialOrd)]
enum NaturalPart {
    Number(u64),
    Text(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mobi_unset_content_bounds_fall_back_to_palmdoc_record_count() {
        assert_eq!(mobi_text_record_bounds(u16::MAX, 174, 185), Ok((1, 175)));
        assert_eq!(mobi_text_record_bounds(2, 3, 8), Ok((2, 5)));
        assert!(mobi_text_record_bounds(u16::MAX, 8, 4).is_err());
    }

    #[test]
    fn palmdoc_decoder_handles_literals_back_references_and_space_codes() {
        let mut output = Vec::new();
        palmdoc_decompress_into(b"abc\x80\x18\xc1", &mut output, 32).unwrap();
        assert_eq!(output, b"abcabc A");
    }

    #[test]
    fn palmdoc_decoder_rejects_invalid_back_references() {
        let mut output = Vec::new();
        assert!(palmdoc_decompress_into(b"\x80\x18", &mut output, 32).is_err());
    }

    #[test]
    fn mobi_text_record_trailers_are_removed_before_decompression() {
        assert_eq!(trim_mobi_trailing_data(b"text\0\x81", 0b11).unwrap(), b"text");
        assert_eq!(trim_mobi_trailing_data(b"text", 0).unwrap(), b"text");
        assert!(trim_mobi_trailing_data(b"\x81", 0b11).is_err());
    }

    #[test]
    fn virtual_resolution_stays_inside_publication_namespace() {
        assert_eq!(resolve_virtual("cbz/pages/4.html", "../images/4"), "cbz/images/4");
        assert_eq!(resolve_virtual("fb2/book.xhtml", "resources/cover"), "fb2/resources/cover");
    }

    #[test]
    fn cbz_page_names_sort_naturally() {
        let mut names = ["10.jpg", "2.jpg", "1.jpg"];
        names.sort_by_key(|name| natural_key(name));
        assert_eq!(names, ["1.jpg", "2.jpg", "10.jpg"]);
    }

    #[test]
    fn fb2_is_exposed_as_virtual_html_and_embedded_resources() {
        let xml = br##"<?xml version="1.0" encoding="utf-8"?>
            <FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" xmlns:l="http://www.w3.org/1999/xlink">
              <description><title-info><genre>prose</genre><author><first-name>Ada</first-name><last-name>Lovelace</last-name></author><book-title>Example</book-title><lang>en</lang></title-info></description>
              <body><section id="chapter"><title><p>Chapter</p></title><p>Hello <emphasis>world</emphasis>.</p><image l:href="#cover"/></section></body>
              <binary id="cover" content-type="image/gif">R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==</binary>
            </FictionBook>"##;
        let publication = PublicationSource::fb2(Path::new("example.fb2"), xml).unwrap();
        assert_eq!(publication.metadata.title, "Example");
        assert_eq!(publication.metadata.creators[0].name, "Ada Lovelace");
        let html = publication.provider.read_string("fb2/book.xhtml").unwrap();
        assert!(html.contains("<em>world</em>"));
        assert!(publication.provider.read_bytes("fb2/resources/cover").unwrap().starts_with(b"GIF"));
        let toc = publication.provider.toc().unwrap().unwrap();
        assert_eq!(toc[0].title, "Chapter");
        assert_eq!(toc[0].link, "fb2/book.xhtml#chapter");
    }

    #[test]
    fn cbz_images_are_loaded_lazily_through_virtual_page_uris() {
        use std::io::Write;
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("10.gif", options).unwrap();
        writer.write_all(b"GIF89a").unwrap();
        writer.start_file("2.gif", options).unwrap();
        writer.write_all(b"GIF89a").unwrap();
        writer.start_file("ComicInfo.xml", options).unwrap();
        writer.write_all(br#"<ComicInfo><Title>Example Comic</Title><Writer>Ada, Grace</Writer><LanguageISO>en</LanguageISO><Manga>YesAndRightToLeft</Manga><Pages><Page Image="1" Type="FrontCover"/></Pages></ComicInfo>"#).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        let publication = PublicationSource::cbz(Path::new("comic.cbz"), bytes).unwrap();
        assert_eq!(publication.document_uris, ["cbz/pages/0.html", "cbz/pages/1.html"]);
        assert_eq!(publication.provider.read_bytes("cbz/images/0").unwrap(), b"GIF89a");
        assert_eq!(publication.metadata.title, "Example Comic");
        assert_eq!(publication.metadata.creators.len(), 2);
        assert_eq!(publication.provider.toc().unwrap().unwrap().len(), 2);
        assert_eq!(publication.layout, PublicationLayout::PagedImages { page_count: 2, reading_direction: ReadingDirection::RightToLeft, cover_resource: Some("cbz/images/1".to_owned()) });
    }

    #[test]
    fn standalone_html_keeps_relative_resources_inside_its_directory() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("document");
        std::fs::create_dir(&root).unwrap();
        let html_path = root.join("essay.html");
        let css_path = root.join("style.css");
        std::fs::write(&html_path, b"<!doctype html><html><link rel=\"stylesheet\" href=\"style.css\"><body>Essay</body></html>").unwrap();
        std::fs::write(&css_path, b"body { color: black }").unwrap();
        std::fs::write(parent.path().join("outside.css"), b"secret").unwrap();

        let publication = PublicationSource::html(&html_path).unwrap();
        let base = &publication.document_uris[0];
        let allowed = publication.provider.resolve(base, "style.css");
        assert_eq!(publication.provider.read_bytes(&allowed).unwrap(), b"body { color: black }");
        let blocked = publication.provider.resolve(base, "../outside.css");
        assert!(!publication.provider.exists(&blocked));
    }

    #[test]
    fn webpub_contains_article_metadata_and_only_reachable_local_assets() {
        let root = tempfile::tempdir().unwrap();
        let html = root.path().join("essay.html");
        std::fs::create_dir(root.path().join("styles")).unwrap();
        std::fs::create_dir(root.path().join("images")).unwrap();
        std::fs::write(&html, br#"<!doctype html><html lang="en"><head><title>Bundled</title><meta name="author" content="Ada Lovelace"><meta property="og:site_name" content="Analytical Review"><meta property="article:published_time" content="1843-01-01"><link rel="canonical" href="https://example.test/essay"><link rel="stylesheet" href="styles/book.css"></head><body><article><img src="images/cover.png"></article></body></html>"#).unwrap();
        std::fs::write(root.path().join("styles/book.css"), b"body { background-image: url('../images/paper.png') }").unwrap();
        std::fs::write(root.path().join("images/cover.png"), b"cover").unwrap();
        std::fs::write(root.path().join("images/paper.png"), b"paper").unwrap();
        std::fs::write(root.path().join("private.txt"), b"not part of the publication").unwrap();

        let bytes = bundle_webpub(&html).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.clone())).unwrap();
        let manifest: serde_json::Value = serde_json::from_reader(archive.by_name("manifest.json").unwrap()).unwrap();
        assert_eq!(manifest["metadata"]["@type"], "https://schema.org/Article");
        assert_eq!(manifest["metadata"]["author"][0], "Ada Lovelace");
        assert_eq!(manifest["metadata"]["publisher"], "Analytical Review");
        assert_eq!(manifest["metadata"]["published"], "1843-01-01");
        assert_eq!(manifest["links"][0]["rel"], "self");
        assert_eq!(manifest["links"][1]["href"], "https://example.test/essay");
        assert_eq!(manifest["readingOrder"][0]["href"], "essay.html");
        assert_eq!(archive.by_name("styles/book.css").unwrap().compression(), zip::CompressionMethod::Deflated);
        assert_eq!(archive.by_name("images/cover.png").unwrap().compression(), zip::CompressionMethod::Stored);
        drop(archive);
        let publication = PublicationSource::webpub(bytes).unwrap();

        assert_eq!(publication.metadata.title, "Bundled");
        assert_eq!(publication.metadata.creators[0].name, "Ada Lovelace");
        assert_eq!(publication.provider.read_bytes("images/cover.png").unwrap(), b"cover");
        assert_eq!(publication.provider.read_bytes("images/paper.png").unwrap(), b"paper");
        assert!(!publication.provider.exists("private.txt"));
        assert!(publication.provider.read_string("essay.html").unwrap().contains("styles/book.css"));
    }

    #[test]
    #[ignore = "set BOKHEIM_MOBI_FIXTURE to a DRM-free real-world MOBI file"]
    fn probes_real_mobi_fixture() {
        let path = std::env::var_os("BOKHEIM_MOBI_FIXTURE").map(PathBuf::from).expect("BOKHEIM_MOBI_FIXTURE is set");
        let bytes = std::fs::read(&path).unwrap();
        let publication = PublicationSource::mobi(&path, &bytes).unwrap();
        assert!(!publication.document_uris.is_empty());
        let first = publication.provider.read_string(&publication.document_uris[0]).unwrap();
        assert!(first.contains("<body>"));
        assert!(first.len() > 100);
    }
}
