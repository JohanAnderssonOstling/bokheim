//! Interoperable PDF text-markup annotations.
//!
//! Bokheim stores synchronization state in SQLite, but materializes PDF marks
//! using the standard annotation dictionary fields understood by other PDF
//! readers. Existing annotations are parsed without rewriting them.

use library_model::{AnnotationStyle, PdfAnnotationRect};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, StringFormat};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use sync_common::ContentHash;

const BOKHEIM_ANNOTATION_PREFIX: &str = "bokheim:";
const HIGHLIGHT_OPACITY: f32 = 0.38;

pub const PDF_BASE_HASH_KEY: &[u8] = b"BokheimContentHash";

/// Canonical content identity. A PDF adopted by Bokheim carries the raw
/// BLAKE3 of its pristine base file so annotation writes do not change the
/// publication identity.
pub fn generate_content_hash_blake3(path: &Path, _buffer: &mut [u8]) -> std::io::Result<ContentHash> {
    if path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("pdf")) {
        match Document::load(path) {
            Ok(document) if document.is_encrypted() => {
                return Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "encrypted PDFs are not supported"));
            }
            Ok(document) => {
                if let Some(hash) = embedded_pdf_base_hash(&document) {
                    return Ok(hash);
                }
            }
            Err(_) => {}
        }
    }
    raw_file_hash(path)
}

pub fn raw_file_hash(path: &Path) -> std::io::Result<ContentHash> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(ContentHash::new(&hasher.finalize().to_hex().to_string()))
}

fn embedded_pdf_base_hash(document: &Document) -> Option<ContentHash> {
    let root_id = document.trailer.get(b"Root").ok()?.as_reference().ok()?;
    let catalog = document.get_object(root_id).ok()?.as_dict().ok()?;
    let bytes = catalog.get(PDF_BASE_HASH_KEY).ok()?.as_str().ok()?;
    let value = std::str::from_utf8(bytes).ok()?;
    (value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())).then(|| ContentHash::new(&value.to_ascii_lowercase()))
}

#[derive(Clone, Debug)]
pub struct ParsedPdfAnnotation {
    pub page_index: u32,
    pub annotation_index: usize,
    pub name: Option<String>,
    pub bokheim_id: Option<String>,
    pub style: AnnotationStyle,
    pub rects: Vec<PdfAnnotationRect>,
    pub color: String,
    pub opacity: f32,
    pub note: String,
    pub author: String,
}

/// Application-neutral annotation data required to materialize a PDF text
/// markup annotation.
pub struct PdfAnnotation {
    pub id: String,
    pub content_hash: ContentHash,
    pub page_index: u32,
    pub rects: Vec<PdfAnnotationRect>,
    pub style: AnnotationStyle,
    pub color: String,
    pub note: String,
}

pub fn parse(path: &Path) -> Result<Vec<ParsedPdfAnnotation>, Box<dyn std::error::Error>> {
    let document = Document::load(path)?;
    if document.is_encrypted() {
        return Err("encrypted PDFs are not supported".into());
    }
    Ok(parse_document(&document))
}

/// Preserves the exact first-import bytes before a writable working PDF is
/// changed. This must be a real copy, never a hard link to the working file.
pub fn stage_base(staging_dir: &Path, working_path: &Path, content_hash: &ContentHash) -> Result<PathBuf, Box<dyn std::error::Error>> {
    fs::create_dir_all(&staging_dir)?;
    let destination = staging_dir.join(format!("{}.pdf", content_hash.as_str()));
    if destination.is_file() {
        verify_base_hash(&destination, content_hash)?;
        return Ok(destination);
    }
    verify_base_hash(working_path, content_hash)?;
    let temporary = staging_dir.join(format!(".{}.{}.partial", content_hash.as_str(), uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        fs::copy(working_path, &temporary)?;
        fs::OpenOptions::new().read(true).open(&temporary)?.sync_all()?;
        verify_base_hash(&temporary, content_hash)?;
        fs::rename(&temporary, &destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(destination)
}

pub fn is_staged_base(staging_dir: &Path, path: &Path) -> bool {
    path.parent() == Some(staging_dir)
}

fn verify_base_hash(path: &Path, expected: &ContentHash) -> Result<(), Box<dyn std::error::Error>> {
    let actual = raw_file_hash(path)?;
    if &actual != expected {
        return Err(format!("pristine PDF base hash mismatch at {}: expected {expected}, found {actual}", path.display()).into());
    }
    Ok(())
}

pub fn upsert(path: &Path, annotation: &PdfAnnotation) -> Result<(), Box<dyn std::error::Error>> {
    let mut document = Document::load(path)?;
    if document.is_encrypted() {
        return Err("encrypted PDFs are not supported".into());
    }
    ensure_base_hash_marker(&mut document, annotation.content_hash.as_str())?;
    let page_id = document.get_pages().get(&(annotation.page_index + 1)).copied().ok_or_else(|| format!("PDF page {} does not exist", annotation.page_index + 1))?;
    let page_box = page_box(&document, page_id).ok_or("PDF page has no valid crop or media box")?;
    let name = format!("{BOKHEIM_ANNOTATION_PREFIX}{}", annotation.id);
    let replacement = annotation_dictionary(annotation, &annotation.rects, page_box, &name)?;

    if let Some(annotation_id) = find_named_annotation(&document, page_id, &name) {
        // An existing appearance stream describes the old style, colour, or
        // geometry. Remove it only because this annotation is being edited;
        // untouched third-party appearance streams remain byte-for-byte data.
        *document.get_object_mut(annotation_id)?.as_dict_mut()? = replacement;
    } else {
        let annotation_id = document.add_object(replacement);
        append_page_annotation(&mut document, page_id, annotation_id)?;
    }
    save_atomically(&mut document, path)
}

fn ensure_base_hash_marker(document: &mut Document, content_hash: &str) -> Result<(), Box<dyn std::error::Error>> {
    let root_id = document.trailer.get(b"Root")?.as_reference()?;
    let catalog = document.get_object_mut(root_id)?.as_dict_mut()?;
    if let Ok(existing) = catalog.get(PDF_BASE_HASH_KEY) {
        let existing = pdf_string(existing).ok_or("PDF contains an invalid Bokheim base hash")?;
        if !existing.eq_ignore_ascii_case(content_hash) {
            return Err(format!("PDF base hash marker mismatch: expected {content_hash}, found {existing}").into());
        }
        return Ok(());
    }
    catalog.set(PDF_BASE_HASH_KEY, pdf_text(content_hash));
    Ok(())
}

pub fn remove(path: &Path, annotation_id: &str) -> Result<bool, Box<dyn std::error::Error>> {
    let mut document = Document::load(path)?;
    if document.is_encrypted() {
        return Err("encrypted PDFs are not supported".into());
    }
    let name = format!("{BOKHEIM_ANNOTATION_PREFIX}{annotation_id}");
    let pages = document.get_pages();
    for page_id in pages.into_values() {
        let Some(mut annotations) = page_annotations(&document, page_id) else { continue };
        let original_len = annotations.len();
        annotations.retain(|object| annotation_name(&document, object).as_deref() != Some(name.as_str()));
        if annotations.len() == original_len {
            continue;
        }
        document.get_object_mut(page_id)?.as_dict_mut()?.set("Annots", Object::Array(annotations));
        save_atomically(&mut document, path)?;
        return Ok(true);
    }
    Ok(false)
}

fn parse_document(document: &Document) -> Vec<ParsedPdfAnnotation> {
    let mut parsed = Vec::new();
    for (page_number, page_id) in document.get_pages() {
        let Some(page_box) = page_box(document, page_id) else { continue };
        let Some(annotations) = page_annotations(document, page_id) else { continue };
        for (annotation_index, object) in annotations.iter().enumerate() {
            let Some(dictionary) = annotation_dictionary_ref(document, object) else { continue };
            let Some(style) = annotation_style(dictionary) else { continue };
            let rects = annotation_rects(document, dictionary, page_box);
            if rects.is_empty() {
                continue;
            }
            let name = dictionary.get(b"NM").ok().and_then(pdf_string);
            let bokheim_id = name.as_deref().and_then(|name| name.strip_prefix(BOKHEIM_ANNOTATION_PREFIX)).map(str::to_owned);
            let (color, opacity) = annotation_color(document, dictionary, &style);
            parsed.push(ParsedPdfAnnotation {
                page_index: page_number.saturating_sub(1),
                annotation_index,
                name,
                bokheim_id,
                style,
                rects,
                color,
                opacity,
                note: dictionary.get(b"Contents").ok().and_then(pdf_string).unwrap_or_default(),
                author: dictionary.get(b"T").ok().and_then(pdf_string).unwrap_or_default(),
            });
        }
    }
    parsed
}

fn annotation_dictionary(annotation: &PdfAnnotation, rects: &[PdfAnnotationRect], page_box: [f32; 4], name: &str) -> Result<Dictionary, Box<dyn std::error::Error>> {
    if rects.is_empty() {
        return Err("PDF annotation has no rectangles".into());
    }
    let pdf_rects = rects.iter().filter_map(|rect| normalized_to_pdf_rect(*rect, page_box)).collect::<Vec<_>>();
    if pdf_rects.is_empty() {
        return Err("PDF annotation has no valid rectangles".into());
    }
    let left = pdf_rects.iter().map(|rect| rect[0]).fold(f32::INFINITY, f32::min);
    let bottom = pdf_rects.iter().map(|rect| rect[1]).fold(f32::INFINITY, f32::min);
    let right = pdf_rects.iter().map(|rect| rect[2]).fold(f32::NEG_INFINITY, f32::max);
    let top = pdf_rects.iter().map(|rect| rect[3]).fold(f32::NEG_INFINITY, f32::max);
    let mut quad_points = Vec::with_capacity(pdf_rects.len() * 8);
    for [left, bottom, right, top] in pdf_rects {
        // ISO 32000 text-markup quadrilateral order: top-left, top-right,
        // bottom-left, bottom-right. This is also what PDFium emits.
        quad_points.extend([left, top, right, top, left, bottom, right, bottom].map(real));
    }
    let [red, green, blue] = parse_hex_rgb(&annotation.color).unwrap_or([0xf6, 0xc9, 0x45]);
    let opacity = match annotation.style {
        AnnotationStyle::Highlight => HIGHLIGHT_OPACITY,
        AnnotationStyle::Underline | AnnotationStyle::Squiggly | AnnotationStyle::Strikethrough => 1.0,
    };
    let subtype = match annotation.style {
        AnnotationStyle::Highlight => "Highlight",
        AnnotationStyle::Underline => "Underline",
        AnnotationStyle::Squiggly => "Squiggly",
        AnnotationStyle::Strikethrough => "StrikeOut",
    };
    Ok(dictionary! {
        "Type" => "Annot",
        "Subtype" => subtype,
        "Rect" => vec![real(left), real(bottom), real(right), real(top)],
        "QuadPoints" => Object::Array(quad_points),
        "C" => vec![real(red as f32 / 255.0), real(green as f32 / 255.0), real(blue as f32 / 255.0)],
        "CA" => real(opacity),
        "Contents" => pdf_text(&annotation.note),
        "T" => pdf_text("Bokheim"),
        "NM" => pdf_text(name),
        // Print flag. The annotation remains visible in ordinary readers and
        // is included when they print the document.
        "F" => 4,
    })
}

fn annotation_style(dictionary: &Dictionary) -> Option<AnnotationStyle> {
    match dictionary.get(b"Subtype").ok()?.as_name().ok()? {
        b"Highlight" => Some(AnnotationStyle::Highlight),
        b"Underline" => Some(AnnotationStyle::Underline),
        b"Squiggly" => Some(AnnotationStyle::Squiggly),
        b"StrikeOut" => Some(AnnotationStyle::Strikethrough),
        _ => None,
    }
}

fn annotation_color(document: &Document, dictionary: &Dictionary, style: &AnnotationStyle) -> (String, f32) {
    let components = dictionary.get_deref(b"C", document).ok().and_then(|object| object.as_array().ok()).map(|values| values.iter().filter_map(|value| value.as_float().ok()).collect::<Vec<_>>()).unwrap_or_default();
    let rgb = match components.as_slice() {
        [gray] => {
            let value = unit_byte(*gray);
            [value, value, value]
        }
        [red, green, blue] => [unit_byte(*red), unit_byte(*green), unit_byte(*blue)],
        [cyan, magenta, yellow, black] => {
            let convert = |component: f32| unit_byte(1.0 - (component + *black).min(1.0));
            [convert(*cyan), convert(*magenta), convert(*yellow)]
        }
        _ => [0xf6, 0xc9, 0x45],
    };
    let default_opacity = if matches!(style, AnnotationStyle::Highlight) { HIGHLIGHT_OPACITY } else { 1.0 };
    let opacity = dictionary.get_deref(b"CA", document).ok().and_then(|value| value.as_float().ok()).unwrap_or(default_opacity).clamp(0.0, 1.0);
    (format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]), opacity)
}

fn annotation_rects(document: &Document, dictionary: &Dictionary, page_box: [f32; 4]) -> Vec<PdfAnnotationRect> {
    let quad_values = dictionary.get_deref(b"QuadPoints", document).ok().and_then(|object| object.as_array().ok()).map(|values| values.iter().filter_map(|value| value.as_float().ok()).collect::<Vec<_>>()).unwrap_or_default();
    let rects = quad_values
        .chunks_exact(8)
        .filter_map(|quad| {
            let left = quad.iter().step_by(2).copied().fold(f32::INFINITY, f32::min);
            let right = quad.iter().step_by(2).copied().fold(f32::NEG_INFINITY, f32::max);
            let bottom = quad.iter().skip(1).step_by(2).copied().fold(f32::INFINITY, f32::min);
            let top = quad.iter().skip(1).step_by(2).copied().fold(f32::NEG_INFINITY, f32::max);
            pdf_to_normalized_rect([left, bottom, right, top], page_box)
        })
        .collect::<Vec<_>>();
    if !rects.is_empty() {
        return rects;
    }
    dictionary.get_deref(b"Rect", document).ok().and_then(|object| object.as_array().ok()).and_then(|values| object_rect(values)).and_then(|rect| pdf_to_normalized_rect(rect, page_box)).into_iter().collect()
}

fn page_annotations(document: &Document, page_id: ObjectId) -> Option<Vec<Object>> {
    let page = document.get_object(page_id).ok()?.as_dict().ok()?;
    page.get_deref(b"Annots", document).ok()?.as_array().ok().cloned()
}

fn annotation_dictionary_ref<'a>(document: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    let (_, resolved) = document.dereference(object).ok()?;
    resolved.as_dict().ok()
}

fn annotation_name(document: &Document, object: &Object) -> Option<String> {
    annotation_dictionary_ref(document, object)?.get(b"NM").ok().and_then(pdf_string)
}

fn find_named_annotation(document: &Document, page_id: ObjectId, name: &str) -> Option<ObjectId> {
    page_annotations(document, page_id)?.into_iter().find_map(|object| (annotation_name(document, &object).as_deref() == Some(name)).then(|| object.as_reference().ok()).flatten())
}

fn append_page_annotation(document: &mut Document, page_id: ObjectId, annotation_id: ObjectId) -> Result<(), lopdf::Error> {
    let existing = document.get_object(page_id)?.as_dict()?.get(b"Annots").ok().cloned();
    match existing {
        Some(Object::Reference(array_id)) => document.get_object_mut(array_id)?.as_array_mut()?.push(annotation_id.into()),
        Some(Object::Array(mut annotations)) => {
            annotations.push(annotation_id.into());
            document.get_object_mut(page_id)?.as_dict_mut()?.set("Annots", annotations);
        }
        Some(_) | None => document.get_object_mut(page_id)?.as_dict_mut()?.set("Annots", vec![annotation_id.into()]),
    }
    Ok(())
}

fn page_box(document: &Document, page_id: ObjectId) -> Option<[f32; 4]> {
    for key in [b"CropBox".as_slice(), b"MediaBox".as_slice()] {
        let mut current = page_id;
        for _ in 0..64 {
            let Ok(dictionary) = document.get_object(current).and_then(Object::as_dict) else {
                break;
            };
            if let Ok(value) = dictionary.get_deref(key, document) {
                if let Ok(values) = value.as_array() {
                    if let Some(rect) = object_rect(values) {
                        return Some(rect);
                    }
                }
            }
            let Ok(parent) = dictionary.get(b"Parent").and_then(Object::as_reference) else {
                break;
            };
            current = parent;
        }
    }
    None
}

fn object_rect(values: &[Object]) -> Option<[f32; 4]> {
    let [left, bottom, right, top] = values else { return None };
    Some([left.as_float().ok()?, bottom.as_float().ok()?, right.as_float().ok()?, top.as_float().ok()?])
}

fn normalized_to_pdf_rect(rect: PdfAnnotationRect, page_box: [f32; 4]) -> Option<[f32; 4]> {
    if ![rect.left, rect.top, rect.width, rect.height].into_iter().all(f32::is_finite) || rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let [page_left, page_bottom, page_right, page_top] = page_box;
    let width = page_right - page_left;
    let height = page_top - page_bottom;
    (width > 0.0 && height > 0.0).then(|| {
        let left = page_left + rect.left * width;
        let right = page_left + (rect.left + rect.width) * width;
        let top = page_top - rect.top * height;
        let bottom = page_top - (rect.top + rect.height) * height;
        [left, bottom, right, top]
    })
}

fn pdf_to_normalized_rect(rect: [f32; 4], page_box: [f32; 4]) -> Option<PdfAnnotationRect> {
    let [page_left, page_bottom, page_right, page_top] = page_box;
    let width = page_right - page_left;
    let height = page_top - page_bottom;
    let [left, bottom, right, top] = rect;
    (width > 0.0 && height > 0.0 && right > left && top > bottom).then(|| PdfAnnotationRect::new((left - page_left) / width, (page_top - top) / height, (right - left) / width, (top - bottom) / height))
}

fn parse_hex_rgb(value: &str) -> Option<[u8; 3]> {
    let hex = value.strip_prefix('#')?;
    (hex.len() == 6).then(|| {
        let value = u32::from_str_radix(hex, 16).ok()?;
        Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
    })?
}

fn unit_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn real(value: f32) -> Object {
    Object::Real(value)
}

fn pdf_text(value: &str) -> Object {
    if value.is_ascii() {
        return Object::String(value.as_bytes().to_vec(), StringFormat::Literal);
    }
    let mut bytes = vec![0xfe, 0xff];
    bytes.extend(value.encode_utf16().flat_map(u16::to_be_bytes));
    Object::String(bytes, StringFormat::Hexadecimal)
}

fn pdf_string(object: &Object) -> Option<String> {
    let bytes = object.as_str().ok()?;
    if let Some(body) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let units = body.chunks_exact(2).map(|pair| u16::from_be_bytes([pair[0], pair[1]])).collect::<Vec<_>>();
        return String::from_utf16(&units).ok();
    }
    Some(String::from_utf8_lossy(bytes).into_owned())
}

fn save_atomically(document: &mut Document, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    document.save_to(&mut bytes)?;
    let file_name = path.file_name().and_then(|name| name.to_str()).unwrap_or("document.pdf");
    let temporary = path.with_file_name(format!(".{file_name}.bokheim-{}.partial", uuid::Uuid::new_v4()));
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{dictionary, Stream};

    fn fixture_pdf(path: &Path) {
        let mut document = Document::with_version("1.7");
        let pages_id = document.new_object_id();
        let content_id = document.add_object(Stream::new(dictionary! {}, Vec::new()));
        let page_id = document.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![10.into(), 20.into(), 610.into(), 820.into()],
            "Contents" => content_id,
            "Resources" => dictionary! {},
        });
        document.objects.insert(pages_id, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }));
        let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);
        document.compress();
        document.save(path).unwrap();
    }

    fn annotation(style: AnnotationStyle, color: &str) -> PdfAnnotation {
        PdfAnnotation {
            id: "annotation-id".to_owned(),
            content_hash: sync_common::ContentHash::new(&"1".repeat(64)),
            page_index: 0,
            rects: vec![PdfAnnotationRect::new(0.1, 0.2, 0.3, 0.04)],
            style,
            color: color.to_owned(),
            note: "Interoperable note".to_owned(),
        }
    }

    #[test]
    fn writes_and_parses_standard_rgb_color_and_opacity() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("annotation.pdf");
        fixture_pdf(&path);

        upsert(&path, &annotation(AnnotationStyle::Highlight, "#60a5fa")).unwrap();

        let parsed = parse(&path).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].bokheim_id.as_deref(), Some("annotation-id"));
        assert_eq!(parsed[0].style, AnnotationStyle::Highlight);
        assert_eq!(parsed[0].color, "#60a5fa");
        assert!((parsed[0].opacity - HIGHLIGHT_OPACITY).abs() < 0.001);
        assert_eq!(parsed[0].note, "Interoperable note");

        let document = Document::load(&path).unwrap();
        let page_id = *document.get_pages().values().next().unwrap();
        let annotations = page_annotations(&document, page_id).unwrap();
        let dictionary = annotation_dictionary_ref(&document, &annotations[0]).unwrap();
        assert_eq!(dictionary.get(b"Subtype").unwrap().as_name().unwrap(), b"Highlight");
        assert_eq!(annotation_name(&document, &annotations[0]).as_deref(), Some("bokheim:annotation-id"));
        assert!(dictionary.get(b"C").unwrap().as_array().is_ok());
        assert!(dictionary.get(b"CA").unwrap().as_float().is_ok());
    }

    #[test]
    fn parses_standard_gray_and_cmyk_colors() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("colors.pdf");
        fixture_pdf(&path);
        upsert(&path, &annotation(AnnotationStyle::Underline, "#ffffff")).unwrap();
        let mut document = Document::load(&path).unwrap();
        let page_id = *document.get_pages().values().next().unwrap();
        let annotation_id = page_annotations(&document, page_id).unwrap()[0].as_reference().unwrap();
        let dictionary = document.get_object_mut(annotation_id).unwrap().as_dict_mut().unwrap();
        dictionary.set("C", vec![real(0.5)]);
        document.save(&path).unwrap();
        assert_eq!(parse(&path).unwrap()[0].color, "#808080");

        let mut document = Document::load(&path).unwrap();
        let annotation_id = page_annotations(&document, page_id).unwrap()[0].as_reference().unwrap();
        document.get_object_mut(annotation_id).unwrap().as_dict_mut().unwrap().set("C", vec![real(1.0), real(0.0), real(0.0), real(0.0)]);
        document.save(&path).unwrap();
        assert_eq!(parse(&path).unwrap()[0].color, "#00ffff");
    }

    #[test]
    fn editing_removes_only_the_edited_stale_appearance() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("appearance.pdf");
        fixture_pdf(&path);
        upsert(&path, &annotation(AnnotationStyle::Highlight, "#f6c945")).unwrap();
        let mut document = Document::load(&path).unwrap();
        let page_id = *document.get_pages().values().next().unwrap();
        let annotation_id = page_annotations(&document, page_id).unwrap()[0].as_reference().unwrap();
        document.get_object_mut(annotation_id).unwrap().as_dict_mut().unwrap().set("AP", dictionary! { "N" => Object::Null });
        document.save(&path).unwrap();

        // Reading does not rewrite or remove an existing appearance stream.
        let _ = parse(&path).unwrap();
        let document = Document::load(&path).unwrap();
        assert!(document.get_object(annotation_id).unwrap().as_dict().unwrap().has(b"AP"));

        upsert(&path, &annotation(AnnotationStyle::Underline, "#4ade80")).unwrap();
        let document = Document::load(&path).unwrap();
        assert!(!document.get_object(annotation_id).unwrap().as_dict().unwrap().has(b"AP"));
    }
}
