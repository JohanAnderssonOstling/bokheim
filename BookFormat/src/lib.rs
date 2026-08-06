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

pub const MAX_BOOK_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const MAX_DOCUMENT_BYTES: u64 = 1024 * 1024 * 1024;
pub const SIGNATURE_PROBE_BYTES: usize = 80;
const M4B_IDENTITY_DOMAIN: &[u8] = b"bokheim-m4b-sampled-blake3-v1";
const M4B_SAMPLE_BYTES: u64 = 128 * 1024;
const M4B_SAMPLE_COUNT: u64 = 10;
const MAX_EPUB_ENTRIES: usize = 20_000;
const MAX_EPUB_UNCOMPRESSED_BYTES: u64 = 2 * MAX_DOCUMENT_BYTES;
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
    Mobi,
    Fb2,
    Cbz,
    Html,
    WebPub,
    M4b,
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
    #[error("unsupported top-level book format; accepted formats are EPUB, PDF, DRM-free MOBI/KF8, FB2, CBZ, HTML, Readium Web Publications, and M4B audiobooks")]
    UnsupportedFormat,
    #[error("invalid EPUB: {0}")]
    InvalidEpub(String),
    #[error("invalid Web Publication: {0}")]
    InvalidWebPub(String),
    #[error("invalid or unsafe PDF: {0}")]
    InvalidPdf(String),
    #[error("invalid M4B audiobook: {0}")]
    InvalidM4b(String),
}

/// Cheap top-level format admission used while an upload is still streaming.
/// Structural validation remains mandatory after the complete file is staged.
pub fn has_supported_signature(signature: &[u8]) -> bool {
    (signature.len() >= 4 && matches!(&signature[..4], b"PK\x03\x04" | b"PK\x05\x06" | b"PK\x07\x08"))
        || (signature.len() >= 5 && &signature[..5] == b"%PDF-")
        || (signature.len() >= 68 && &signature[60..68] == b"BOOKMOBI")
        || (signature.len() >= 12 && &signature[4..8] == b"ftyp")
        || signature.iter().skip_while(|byte| byte.is_ascii_whitespace()).next() == Some(&b'<')
}

/// Validates the bytes at `path`; the file extension and caller-supplied MIME
/// type deliberately do not participate in format detection.
pub fn validate_path(path: impl AsRef<Path>) -> Result<ValidatedBook, ValidationError> {
    let path = path.as_ref();
    let size = std::fs::metadata(path)?.len();
    let mut file = File::open(path)?;
    let mut signature = [0_u8; SIGNATURE_PROBE_BYTES];
    let count = file.read(&mut signature)?;
    file.seek(SeekFrom::Start(0))?;
    let is_m4b = count >= 12 && &signature[4..8] == b"ftyp";
    let size_limit = if is_m4b { MAX_BOOK_BYTES } else { MAX_DOCUMENT_BYTES };
    if size > size_limit {
        return Err(ValidationError::TooLarge { limit: size_limit });
    }
    if !has_supported_signature(&signature[..count]) {
        return Err(ValidationError::UnsupportedFormat);
    }

    let format = if count >= 4 && matches!(&signature[..4], b"PK\x03\x04" | b"PK\x05\x06" | b"PK\x07\x08") {
        validate_zip_publication(file)?
    } else if count >= 5 && &signature[..5] == b"%PDF-" {
        validate_pdf(path)?;
        BookFormat::Pdf
    } else if count >= 68 && &signature[60..68] == b"BOOKMOBI" {
        validate_mobi(file)?;
        BookFormat::Mobi
    } else if is_m4b {
        mp4ameta::Tag::read_from_path(path).map_err(|error| ValidationError::InvalidM4b(error.to_string()))?;
        BookFormat::M4b
    } else {
        validate_markup(file)?
    };

    Ok(ValidatedBook { format, size })
}

/// Returns the canonical content identity. Documents retain their full-file
/// BLAKE3 identity; M4B audiobooks use size, duration, and distributed samples.
pub fn content_identity_for_path(path: impl AsRef<Path>, format: BookFormat) -> Result<String, ValidationError> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    content_identity(&mut file, size, format)
}

/// Byte-oriented equivalent used by browser and HTTP adapters.
pub fn content_identity_for_bytes(bytes: &[u8]) -> Result<String, ValidationError> {
    let format = if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" { BookFormat::M4b } else { BookFormat::Epub };
    content_identity(&mut Cursor::new(bytes), bytes.len() as u64, format)
}

fn content_identity(reader: &mut (impl Read + Seek), size: u64, format: BookFormat) -> Result<String, ValidationError> {
    if format != BookFormat::M4b {
        let mut hasher = blake3::Hasher::new();
        let mut buffer = [0_u8; 1024 * 1024];
        reader.seek(SeekFrom::Start(0))?;
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        return Ok(hasher.finalize().to_hex().to_string());
    }

    reader.seek(SeekFrom::Start(0))?;
    let duration = mp4ameta::Tag::read_from(&mut *reader).map_err(|error| ValidationError::InvalidM4b(error.to_string()))?.duration();
    let mut hasher = blake3::Hasher::new();
    hasher.update(M4B_IDENTITY_DOMAIN);
    hasher.update(&size.to_le_bytes());
    hasher.update(&duration.as_nanos().to_le_bytes());

    let max_start = size.saturating_sub(M4B_SAMPLE_BYTES);
    let mut buffer = vec![0_u8; M4B_SAMPLE_BYTES as usize];
    for index in 0..M4B_SAMPLE_COUNT {
        let offset = ((max_start as u128 * index as u128) / (M4B_SAMPLE_COUNT - 1) as u128) as u64;
        let sample_len = M4B_SAMPLE_BYTES.min(size.saturating_sub(offset)) as usize;
        reader.seek(SeekFrom::Start(offset))?;
        reader.read_exact(&mut buffer[..sample_len])?;
        hasher.update(&offset.to_le_bytes());
        hasher.update(&(sample_len as u64).to_le_bytes());
        hasher.update(&buffer[..sample_len]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

fn validate_zip_publication(mut file: File) -> Result<BookFormat, ValidationError> {
    let (is_epub, is_webpub) = {
        let mut archive = ZipArchive::new(&mut file).map_err(epub)?;
        let names = (0..archive.len()).filter_map(|index| archive.by_index_raw(index).ok().map(|entry| entry.name().to_owned())).collect::<BTreeSet<_>>();
        (names.contains("mimetype"), names.contains("manifest.json"))
    };
    file.seek(SeekFrom::Start(0))?;
    if is_epub {
        validate_epub(file)?;
        Ok(BookFormat::Epub)
    } else if is_webpub {
        validate_webpub(file)?;
        Ok(BookFormat::WebPub)
    } else {
        validate_cbz(file)?;
        Ok(BookFormat::Cbz)
    }
}

fn validate_webpub(file: File) -> Result<(), ValidationError> {
    validate_webpub_inner(file).map_err(|error| match error {
        ValidationError::InvalidEpub(message) => ValidationError::InvalidWebPub(message),
        error => error,
    })
}

fn validate_webpub_inner(file: File) -> Result<(), ValidationError> {
    let mut archive = ZipArchive::new(file).map_err(|error| epub(format!("invalid Web Publication: {error}")))?;
    if archive.is_empty() || archive.len() > MAX_EPUB_ENTRIES {
        return Err(epub(format!("Web Publication entry count {} is outside the supported range", archive.len())));
    }

    let mut names = BTreeSet::new();
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let raw = archive.by_index_raw(index).map_err(epub)?;
        if raw.encrypted() {
            return Err(epub(format!("encrypted Web Publication entry '{}' is not accepted", raw.name())));
        }
        drop(raw);
        let entry = archive.by_index(index).map_err(epub)?;
        let raw_name = entry.name().trim_end_matches('/');
        if raw_name.is_empty() {
            continue;
        }
        let name = canonical_archive_path(raw_name).map_err(epub)?;
        if entry.is_dir() {
            continue;
        }
        if !names.insert(name.clone()) {
            return Err(epub(format!("Web Publication contains duplicate entry '{name}'")));
        }
        if entry.size() > MAX_RESOURCE_BYTES {
            return Err(epub(format!("Web Publication entry '{name}' exceeds size limits")));
        }
        total = total.checked_add(entry.size()).ok_or_else(|| epub("Web Publication uncompressed size overflow"))?;
        if total > MAX_EPUB_UNCOMPRESSED_BYTES {
            return Err(epub("Web Publication uncompressed size exceeds limits"));
        }
    }

    let mut manifest_entry = archive.by_name("manifest.json").map_err(|_| epub("Web Publication manifest.json is missing"))?;
    if manifest_entry.size() > MAX_XML_BYTES {
        return Err(epub("Web Publication manifest exceeds size limits"));
    }
    let mut manifest_bytes = Vec::with_capacity(manifest_entry.size() as usize);
    manifest_entry.read_to_end(&mut manifest_bytes)?;
    drop(manifest_entry);
    let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes).map_err(|error| epub(format!("invalid Web Publication manifest: {error}")))?;
    let context_valid = match manifest.get("@context") {
        Some(serde_json::Value::String(value)) => value == "https://readium.org/webpub-manifest/context.jsonld",
        Some(serde_json::Value::Array(values)) => values.iter().any(|value| value.as_str() == Some("https://readium.org/webpub-manifest/context.jsonld")),
        _ => false,
    };
    if !context_valid {
        return Err(epub("Web Publication manifest has no Readium context"));
    }
    let metadata = manifest.get("metadata").and_then(serde_json::Value::as_object).ok_or_else(|| epub("Web Publication manifest has no metadata object"))?;
    if metadata.get("title").and_then(serde_json::Value::as_str).is_none_or(|title| title.trim().is_empty()) {
        return Err(epub("Web Publication metadata has no title"));
    }
    let links = manifest.get("links").and_then(serde_json::Value::as_array).ok_or_else(|| epub("Web Publication manifest has no links collection"))?;
    let has_self = links.iter().any(|link| {
        let Some(rel) = link.get("rel") else { return false };
        rel.as_str() == Some("self") || rel.as_array().is_some_and(|values| values.iter().any(|value| value.as_str() == Some("self")))
    });
    if !has_self {
        return Err(epub("Web Publication manifest has no self link"));
    }
    let reading_order = manifest.get("readingOrder").and_then(serde_json::Value::as_array).filter(|links| !links.is_empty()).ok_or_else(|| epub("Web Publication manifest has no readingOrder"))?;
    let resources = manifest.get("resources").and_then(serde_json::Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let mut declared = BTreeSet::from(["manifest.json".to_owned()]);
    let mut documents = Vec::new();
    for (link, in_reading_order) in reading_order.iter().map(|link| (link, true)).chain(resources.iter().map(|link| (link, false))) {
        let link = link.as_object().ok_or_else(|| epub("Web Publication resource link is not an object"))?;
        let href = link.get("href").and_then(serde_json::Value::as_str).ok_or_else(|| epub("Web Publication resource has no href"))?;
        let media_type = link.get("type").and_then(serde_json::Value::as_str).ok_or_else(|| epub("Web Publication resource has no media type"))?;
        let href_without_suffix = href.split(['?', '#']).next().unwrap_or(href);
        let decoded = percent_decode_str(href_without_suffix).decode_utf8().map_err(epub)?;
        let path = canonical_archive_path(&decoded).map_err(epub)?;
        if !declared.insert(path.clone()) {
            return Err(epub(format!("Web Publication resource '{path}' is declared more than once")));
        }
        if !names.contains(&path) {
            return Err(epub(format!("Web Publication resource '{path}' is missing from the archive")));
        }
        if in_reading_order {
            if !matches!(media_type, "text/html" | "application/xhtml+xml") {
                return Err(epub("this reader requires HTML resources in Web Publication readingOrder"));
            }
            documents.push(path);
        }
    }
    if names.iter().any(|name| !declared.contains(name)) {
        return Err(epub("Web Publication archive contains an undeclared resource"));
    }
    for document in documents {
        let mut html_entry = archive.by_name(&document).map_err(epub)?;
        if html_entry.size() > MAX_XML_BYTES {
            return Err(epub(format!("Web Publication document '{document}' exceeds the markup size limit")));
        }
        let mut prefix = vec![0_u8; usize::try_from(html_entry.size().min(4096)).unwrap_or(4096)];
        let count = html_entry.read(&mut prefix)?;
        let prefix = String::from_utf8_lossy(&prefix[..count]).to_ascii_lowercase();
        if !prefix.contains("<!doctype html") && !contains_html_root(&prefix) {
            return Err(epub(format!("Web Publication readingOrder resource '{document}' is not HTML")));
        }
    }
    Ok(())
}

fn validate_cbz(file: File) -> Result<(), ValidationError> {
    let mut archive = ZipArchive::new(file).map_err(|error| epub(format!("invalid CBZ archive: {error}")))?;
    if archive.is_empty() || archive.len() > MAX_EPUB_ENTRIES {
        return Err(epub(format!("CBZ entry count {} is outside the supported range", archive.len())));
    }
    let mut image_count = 0_usize;
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let raw = archive.by_index_raw(index).map_err(epub)?;
        if raw.encrypted() {
            return Err(epub(format!("encrypted CBZ entry '{}' is not accepted", raw.name())));
        }
        drop(raw);
        let mut entry = archive.by_index(index).map_err(epub)?;
        if entry.is_dir() {
            canonical_archive_path(entry.name().trim_end_matches('/')).map_err(epub)?;
            continue;
        }
        let name = canonical_archive_path(entry.name()).map_err(epub)?;
        total = total.checked_add(entry.size()).ok_or_else(|| epub("CBZ uncompressed size overflow"))?;
        if total > MAX_EPUB_UNCOMPRESSED_BYTES {
            return Err(epub("CBZ uncompressed size exceeds limits"));
        }
        if Path::new(&name).file_name().and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case("ComicInfo.xml")) {
            if entry.size() > MAX_XML_BYTES {
                return Err(epub("CBZ ComicInfo.xml exceeds the markup size limit"));
            }
            let mut bytes = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut bytes)?;
            validate_comic_info(&bytes)?;
            continue;
        }
        if entry.size() > MAX_IMAGE_BYTES {
            return Err(epub(format!("CBZ image '{name}' exceeds size limits")));
        }
        let extension = Path::new(&name).extension().and_then(|value| value.to_str()).unwrap_or_default().to_ascii_lowercase();
        if !matches!(extension.as_str(), "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "avif") {
            return Err(epub(format!("CBZ contains non-image entry '{name}'")));
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        let format = image::guess_format(&bytes).map_err(|error| epub(format!("CBZ image '{name}' is invalid: {error}")))?;
        let (width, height) = ImageReader::with_format(Cursor::new(&bytes), format).into_dimensions().map_err(|error| epub(format!("CBZ image '{name}' dimensions cannot be read: {error}")))?;
        if width == 0 || height == 0 || width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
            return Err(epub(format!("CBZ image '{name}' dimensions {width}x{height} exceed limits")));
        }
        image_count += 1;
    }
    if image_count == 0 {
        return Err(epub("CBZ contains no images"));
    }
    Ok(())
}

fn validate_comic_info(bytes: &[u8]) -> Result<(), ValidationError> {
    let mut reader = Reader::from_reader(bytes);
    reader.trim_text(true);
    let mut buffer = Vec::new();
    let mut root_seen = false;
    loop {
        match reader.read_event_into(&mut buffer).map_err(|error| epub(format!("invalid ComicInfo.xml: {error}")))? {
            Event::Start(element) if !root_seen => {
                if !local_name(element.name().as_ref()).eq_ignore_ascii_case(b"ComicInfo") {
                    return Err(epub("ComicInfo.xml has no ComicInfo root"));
                }
                root_seen = true;
            }
            Event::DocType(_) => return Err(epub("ComicInfo.xml document types are not allowed")),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen {
        return Err(epub("ComicInfo.xml has no ComicInfo root"));
    }
    Ok(())
}

fn validate_markup(file: File) -> Result<BookFormat, ValidationError> {
    let mut bytes = Vec::new();
    file.take(MAX_XML_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_XML_BYTES {
        return Err(epub("HTML/FB2 document exceeds the markup size limit"));
    }
    let prefix_len = bytes.len().min(4096);
    let prefix = String::from_utf8_lossy(&bytes[..prefix_len]).to_ascii_lowercase();
    if prefix.contains("<!doctype html") || contains_html_root(&prefix) {
        return Ok(BookFormat::Html);
    }
    let mut reader = Reader::from_reader(bytes.as_slice());
    reader.trim_text(true);
    let mut buffer = Vec::new();
    let mut root_seen = false;
    loop {
        match reader.read_event_into(&mut buffer).map_err(|error| epub(format!("invalid FB2 XML: {error}")))? {
            Event::Start(element) if !root_seen => {
                if !local_name(element.name().as_ref()).eq_ignore_ascii_case(b"FictionBook") {
                    return Err(ValidationError::UnsupportedFormat);
                }
                root_seen = true;
            }
            Event::DocType(_) => return Err(epub("FB2 document types are not allowed")),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !root_seen {
        return Err(epub("FB2 document has no FictionBook root"));
    }
    Ok(BookFormat::Fb2)
}

fn contains_html_root(prefix: &str) -> bool {
    prefix.match_indices("<html").any(|(offset, _)| prefix.as_bytes().get(offset + 5).is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/')))
}

fn validate_mobi(mut file: File) -> Result<(), ValidationError> {
    let mut header = [0_u8; 82];
    file.read_exact(&mut header)?;
    if &header[60..68] != b"BOOKMOBI" {
        return Err(ValidationError::UnsupportedFormat);
    }
    let record_count = u16::from_be_bytes([header[76], header[77]]);
    if record_count == 0 {
        return Err(epub("MOBI has no Palm database records"));
    }
    let first_record = u32::from_be_bytes([header[78], header[79], header[80], header[81]]) as u64;
    file.seek(SeekFrom::Start(first_record + 12))?;
    let mut encryption = [0_u8; 2];
    file.read_exact(&mut encryption)?;
    if u16::from_be_bytes(encryption) != 0 {
        return Err(epub("DRM-protected MOBI/KF8 publications are not accepted"));
    }
    Ok(())
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
        if entry.is_dir() {
            let directory_name = raw_name.strip_suffix('/').ok_or_else(|| epub(format!("unsafe or non-canonical archive directory '{raw_name}'")))?;
            canonical_archive_path(directory_name).map_err(epub)?;
            continue;
        }
        let name = canonical_archive_path(raw_name).map_err(epub)?;
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

    if archive_files.contains_key("META-INF/encryption.xml") {
        return Err(epub("encrypted EPUB resources are not accepted"));
    }
    let manifest_paths: BTreeSet<_> = manifest.keys().cloned().collect();
    let actual_publication_paths: BTreeSet<_> = archive_files.keys().filter(|path| path.as_str() != "mimetype" && !path.starts_with("META-INF/") && path.as_str() != opf_path).cloned().collect();
    if let Some(path) = manifest_paths.difference(&actual_publication_paths).next() {
        return Err(epub(format!("manifest resource '{path}' is missing from the archive")));
    }

    for item in manifest.values() {
        validate_manifest_resource(&mut archive, item)?;
    }
    for path in actual_publication_paths.difference(&manifest_paths) {
        validate_unmanifested_resource(&mut archive, path)?;
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
        "font/otf"
        | "font/opentype"
        | "font/truetype"
        | "font/ttf"
        | "application/font-sfnt"
        | "application/vnd.ms-opentype"
        | "application/x-font-opentype"
        | "application/x-font-truetype"
        | "application/x-font-ttf"
        | "application/x-truetype-font" => validate_font(archive, item, &[b"OTTO", b"\0\x01\0\0", b"true", b"typ1", b"ttcf"]),
        "font/woff" | "application/font-woff" => validate_font(archive, item, &[b"wOFF"]),
        "font/woff2" => validate_font(archive, item, &[b"wOF2"]),
        "application/xml" | "image/svg+xml" | "text/xml" => {
            let bytes = read_entry(archive, &item.path, MAX_XML_BYTES)?;
            validate_content_xml(&bytes, &item.path)
        }
        media_type => Err(epub(format!("manifest resource '{}' uses prohibited or unsupported media type '{media_type}'", item.path))),
    }
}

fn validate_unmanifested_resource(archive: &mut ZipArchive<File>, path: &str) -> Result<(), ValidationError> {
    let extension = path.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase());
    let media_type = match extension.as_deref() {
        Some("css") => "text/css",
        Some("htm" | "html" | "xhtml") => "application/xhtml+xml",
        Some("ncx") => "application/x-dtbncx+xml",
        Some("otf") => "font/otf",
        Some("ttf") => "font/ttf",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("xml") => "application/xml",
        Some("gif" | "jpeg" | "jpg" | "png" | "svg" | "webp") => return Err(epub(format!("archive image '{path}' is not declared in the package manifest"))),
        _ => return Err(epub(format!("archive resource '{path}' is not declared in the package manifest and has an unsupported type"))),
    };
    validate_manifest_resource(archive, &ManifestItem { path: path.to_owned(), media_type: media_type.to_owned(), properties: BTreeSet::new() })
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
                let link_relation = if name == b"link" { optional_attr(&element, b"rel")?.map(|relation| relation.to_ascii_lowercase()) } else { None };
                for attribute in element.attributes().with_checks(false) {
                    let attribute = attribute.map_err(|error| epub(format!("XML resource '{path}' has an invalid attribute: {error}")))?;
                    let key = local_name(attribute.key.as_ref()).to_ascii_lowercase();
                    if key.starts_with(b"on") {
                        return Err(epub(format!("XML resource '{path}' contains an event-handler attribute")));
                    }
                    let value = attribute.unescape_value().map_err(epub)?.into_owned().trim().to_ascii_lowercase();
                    let embedded_or_executable = ["data:", "file:", "javascript:"].iter().any(|prefix| value.starts_with(prefix));
                    let remote = ["http:", "https:", "//"].iter().any(|prefix| value.starts_with(prefix));
                    let passive_link = name == b"link" && key == b"href" && !link_relation.as_deref().is_some_and(is_active_link_relation);
                    let remote_resource = remote && !(key == b"href" && name == b"a") && !passive_link;
                    let unsafe_style = key == b"style" && ["data:", "file:", "http:", "https:", "javascript:", "//"].iter().any(|needle| value.contains(needle));
                    if (matches!(key.as_slice(), b"src" | b"href" | b"poster" | b"data") && (embedded_or_executable || remote_resource)) || unsafe_style {
                        return Err(epub(format!("XML resource '{path}' contains a remote, embedded, or executable reference")));
                    }
                }
            }
            Event::DocType(doctype) if is_supported_content_doctype(doctype.as_ref()) => {}
            Event::DocType(_) => return Err(epub(format!("XML resource '{path}' contains an unsafe document type"))),
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    Ok(())
}

fn is_supported_content_doctype(doctype: &[u8]) -> bool {
    let start = doctype.iter().position(|byte| !byte.is_ascii_whitespace()).unwrap_or(doctype.len());
    let end = doctype.iter().rposition(|byte| !byte.is_ascii_whitespace()).map_or(start, |index| index + 1);
    let doctype = &doctype[start..end];
    let name_end = doctype.iter().position(|byte| byte.is_ascii_whitespace()).unwrap_or(doctype.len());
    matches_ignore_ascii_case(&doctype[..name_end], &[b"html", b"ncx", b"svg"]) && !doctype[name_end..].iter().any(|byte| matches!(byte, b'[' | b']' | b'<' | b'>'))
}

fn is_active_link_relation(relation: &str) -> bool {
    relation.split_ascii_whitespace().any(|token| matches!(token, "icon" | "modulepreload" | "preload" | "stylesheet"))
}

fn matches_ignore_ascii_case(value: &[u8], candidates: &[&[u8]]) -> bool {
    candidates.iter().any(|candidate| value.eq_ignore_ascii_case(candidate))
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
        epub_file_with_chapter(extra_manifest, extra_files, br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>Book</p></body></html>"#)
    }

    fn epub_file_with_chapter(extra_manifest: &str, extra_files: &[(&str, &[u8])], chapter: &[u8]) -> tempfile::NamedTempFile {
        epub_file_with_layout(extra_manifest, extra_files, chapter, &[])
    }

    fn epub_file_with_layout(extra_manifest: &str, extra_files: &[(&str, &[u8])], chapter: &[u8], directories: &[&str]) -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut writer = ZipWriter::new(file.reopen().unwrap());
        writer.start_file("mimetype", SimpleFileOptions::default().compression_method(CompressionMethod::Stored)).unwrap();
        writer.write_all(b"application/epub+zip").unwrap();
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for directory in directories {
            writer.add_directory(*directory, options).unwrap();
        }
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
        writer.write_all(chapter).unwrap();
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
    fn accepts_source_formats_without_conversion_and_rejects_mobi_drm() {
        let fb2 = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(fb2.path(), br#"<?xml version="1.0"?><FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0"><body><section><p>Text</p></section></body></FictionBook>"#).unwrap();
        assert_eq!(validate_path(fb2.path()).unwrap().format, BookFormat::Fb2);

        let cbz = tempfile::NamedTempFile::new().unwrap();
        let mut writer = ZipWriter::new(cbz.reopen().unwrap());
        writer.start_file("001.png", SimpleFileOptions::default()).unwrap();
        writer.write_all(&png()).unwrap();
        writer.start_file("ComicInfo.xml", SimpleFileOptions::default()).unwrap();
        writer.write_all(br#"<ComicInfo><Title>Example</Title></ComicInfo>"#).unwrap();
        writer.finish().unwrap();
        assert_eq!(validate_path(cbz.path()).unwrap().format, BookFormat::Cbz);

        let mobi = tempfile::NamedTempFile::new().unwrap();
        let mut bytes = vec![0_u8; 128];
        bytes[60..68].copy_from_slice(b"BOOKMOBI");
        bytes[76..78].copy_from_slice(&1_u16.to_be_bytes());
        bytes[78..82].copy_from_slice(&96_u32.to_be_bytes());
        bytes[108..110].copy_from_slice(&0_u16.to_be_bytes());
        std::fs::write(mobi.path(), &bytes).unwrap();
        assert_eq!(validate_path(mobi.path()).unwrap().format, BookFormat::Mobi);
        bytes[108..110].copy_from_slice(&1_u16.to_be_bytes());
        std::fs::write(mobi.path(), bytes).unwrap();
        assert!(validate_path(mobi.path()).unwrap_err().to_string().contains("DRM-protected"));
    }

    #[test]
    fn accepts_html5_and_xhtml_as_native_sources() {
        for source in [b"<!doctype html><html><head><title>Example</title></head><body><p>Hello</p></body></html>".as_slice(), b"<?xml version=\"1.0\"?><html xmlns=\"http://www.w3.org/1999/xhtml\"><body>Hi</body></html>".as_slice()] {
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), source).unwrap();
            assert_eq!(validate_path(file.path()).unwrap().format, BookFormat::Html);
        }
        let not_html = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(not_html.path(), b"<htmlish>not a document</htmlish>").unwrap();
        assert!(matches!(validate_path(not_html.path()), Err(ValidationError::UnsupportedFormat)));
    }

    #[test]
    fn accepts_readium_webpub_and_rejects_unsafe_reading_order() {
        let make_webpub = |entry_point: &str| {
            let file = tempfile::NamedTempFile::new().unwrap();
            let mut writer = ZipWriter::new(file.reopen().unwrap());
            let options = SimpleFileOptions::default();
            writer.start_file("manifest.json", options).unwrap();
            writer
                .write_all(
                    format!(r#"{{"@context":"https://readium.org/webpub-manifest/context.jsonld","metadata":{{"@type":"https://schema.org/Article","title":"Essay"}},"links":[{{"rel":"self","href":"manifest.json","type":"application/webpub+json"}}],"readingOrder":[{{"href":"{entry_point}","type":"text/html"}}],"resources":[{{"href":"assets/book.css","type":"text/css"}}]}}"#).as_bytes(),
                )
                .unwrap();
            writer.start_file("essay.html", options).unwrap();
            writer.write_all(b"<!doctype html><html><body>Text</body></html>").unwrap();
            writer.start_file("assets/book.css", options).unwrap();
            writer.write_all(b"body { color: navy }").unwrap();
            writer.finish().unwrap();
            file
        };

        assert_eq!(validate_path(make_webpub("essay.html").path()).unwrap().format, BookFormat::WebPub);
        assert!(validate_path(make_webpub("../essay.html").path()).unwrap_err().to_string().contains("unsafe or non-canonical archive path"));
    }

    #[test]
    fn accepts_html5_and_legacy_xhtml_doctypes() {
        let chapters: [&[u8]; 2] = [
            br#"<!DOCTYPE html><html xmlns="http://www.w3.org/1999/xhtml"><body><p>Book</p></body></html>"#,
            br#"<!DOCTYPE html PUBLIC '-//W3C//DTD XHTML 1.1//EN' 'http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd'><html xmlns="http://www.w3.org/1999/xhtml"><body/></html>"#,
        ];
        for chapter in chapters {
            let file = epub_file_with_chapter("", &[], chapter);
            assert_eq!(validate_path(file.path()).unwrap().format, BookFormat::Epub);
        }
    }

    #[test]
    fn rejects_embedded_content_doctype_declarations() {
        let file = epub_file_with_chapter("", &[], br#"<!DOCTYPE html [<!ENTITY payload "unsafe">]><html xmlns="http://www.w3.org/1999/xhtml"><body>&payload;</body></html>"#);
        assert!(validate_path(file.path()).unwrap_err().to_string().contains("unsafe document type"));
    }

    #[test]
    fn accepts_external_anchor_links_but_rejects_active_external_resources() {
        for chapter in [
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><a href="https://example.com/book">Source</a></body></html>"#.as_slice(),
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><link itemprop="mainEntityOfPage" href="https://example.com/book"/></body></html>"#.as_slice(),
        ] {
            let file = epub_file_with_chapter("", &[], chapter);
            assert_eq!(validate_path(file.path()).unwrap().format, BookFormat::Epub);
        }

        for chapter in [
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><img src="https://example.com/track.png"/></body></html>"#.as_slice(),
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="https://example.com/style.css"/></head><body/></html>"#.as_slice(),
            br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><a href="javascript:alert(1)">Run</a></body></html>"#.as_slice(),
        ] {
            let file = epub_file_with_chapter("", &[], chapter);
            assert!(validate_path(file.path()).unwrap_err().to_string().contains("remote, embedded, or executable reference"));
        }
    }

    #[test]
    fn accepts_directory_records_and_unmanifested_reader_metadata() {
        let files: &[(&str, &[u8])] = &[("META-INF/calibre_bookmarks.txt", b"{}"), ("META-INF/com.apple.ibooks.display-options.xml", b"<display_options/>")];
        let file = epub_file_with_layout("", files, br#"<html xmlns="http://www.w3.org/1999/xhtml"><body/></html>"#, &["META-INF/", "OEBPS/"]);
        assert_eq!(validate_path(file.path()).unwrap().format, BookFormat::Epub);
    }

    #[test]
    fn rejects_unsafe_directory_records() {
        let file = epub_file_with_layout("", &[], br#"<html xmlns="http://www.w3.org/1999/xhtml"><body/></html>"#, &["../"]);
        assert!(validate_path(file.path()).unwrap_err().to_string().contains("unsafe or non-canonical archive path"));
    }

    #[test]
    fn accepts_legacy_ncx_doctype() {
        let manifest = r#"<item id="toc" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#;
        let ncx = br#"<!DOCTYPE ncx PUBLIC "-//NISO//DTD ncx 2005-1//EN" "http://www.daisy.org/z3986/2005/ncx-2005-1.dtd"><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><navMap/></ncx>"#;
        let file = epub_file(manifest, &[("OEBPS/toc.ncx", ncx)]);
        assert_eq!(validate_path(file.path()).unwrap().format, BookFormat::Epub);
    }

    #[test]
    fn accepts_safe_svg_and_legacy_font_media_types() {
        let manifest = r#"<item id="logo" href="logo.svg" media-type="image/svg+xml"/><item id="font" href="font.ttf" media-type="application/vnd.ms-opentype"/><item id="legacy-font" href="legacy.ttf" media-type="application/x-font-truetype"/><item id="legacy-ttf" href="legacy-x.ttf" media-type="application/x-font-ttf"/>"#;
        let svg = br#"<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN" "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd"><svg xmlns="http://www.w3.org/2000/svg"><rect width="1" height="1"/></svg>"#;
        let file = epub_file(manifest, &[("OEBPS/logo.svg", svg), ("OEBPS/font.ttf", b"\0\x01\0\0font"), ("OEBPS/legacy.ttf", b"truefont"), ("OEBPS/legacy-x.ttf", b"OTTOfont")]);
        assert_eq!(validate_path(file.path()).unwrap().format, BookFormat::Epub);

        let scripted = epub_file(r#"<item id="logo" href="logo.svg" media-type="image/svg+xml"/>"#, &[("OEBPS/logo.svg", b"<svg><script/></svg>" as &[u8])]);
        assert!(validate_path(scripted.path()).unwrap_err().to_string().contains("prohibited element 'script'"));
    }

    #[test]
    fn accepts_safely_validated_undeclared_reader_metadata() {
        let file = epub_file("", &[("OEBPS/accessibility.xml", b"<accessibility/>" as &[u8])]);
        assert_eq!(validate_path(file.path()).unwrap().format, BookFormat::Epub);
    }

    #[test]
    fn rejects_undeclared_archive_images() {
        let image = png();
        let file = epub_file("", &[("OEBPS/hidden.png", &image)]);
        assert!(validate_path(file.path()).unwrap_err().to_string().contains("not declared"));
    }

    #[test]
    fn rejects_unsafe_or_unknown_undeclared_archive_resources() {
        let invalid_image = epub_file("", &[("OEBPS/hidden.png", b"not an image" as &[u8])]);
        assert!(validate_path(invalid_image.path()).unwrap_err().to_string().contains("image"));
        let unknown = epub_file("", &[("OEBPS/payload.bin", b"payload" as &[u8])]);
        assert!(validate_path(unknown.path()).unwrap_err().to_string().contains("unsupported type"));
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
        let error = validate_path(encrypted_zip.path()).unwrap_err().to_string();
        assert!(error.contains("encrypted ZIP entry"), "unexpected validation error: {error}");
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

    #[test]
    fn streaming_signature_probe_matches_every_supported_top_level_shape() {
        let mut mobi = vec![0_u8; SIGNATURE_PROBE_BYTES];
        mobi[60..68].copy_from_slice(b"BOOKMOBI");
        for supported in [b"PK\x03\x04archive".as_slice(), b"%PDF-1.7".as_slice(), mobi.as_slice(), b"  \n<html>".as_slice()] {
            assert!(has_supported_signature(supported));
        }
        for unsupported in [b"\x89PNG\r\n\x1a\nimage".as_slice(), b"\0\0\0\x18ftypmp42video".as_slice(), b"ordinary bytes".as_slice()] {
            assert!(!has_supported_signature(unsupported));
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
