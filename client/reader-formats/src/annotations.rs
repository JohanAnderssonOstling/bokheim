use book_model::{parse_annotation_color, Annotation, AnnotationAnchor, AnnotationStyle, BookId, EpubCfi};
use chrono::DateTime;
use serde_json::Value;
use std::io;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ReaderFormat {
    Foliate,
    Readest,
}

pub fn annotation_id(format: ReaderFormat, publication: &str, book: BookId, source: &str) -> String {
    let namespace = match format {
        ReaderFormat::Foliate => format!("foliate/{publication}/{book}/{source}"),
        ReaderFormat::Readest => format!("readest/{publication}/{source}"),
    };
    Uuid::new_v5(&Uuid::NAMESPACE_URL, namespace.as_bytes()).to_string()
}

fn invalid(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}
fn unsupported(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}

/// Readest names refer to its own palette, rather than CSS named colors.
pub fn readest_color(value: &str) -> io::Result<u32> {
    match value {
        "red" => Ok(0xf87171),
        "yellow" => Ok(0xfacc15),
        "green" => Ok(0x4ade80),
        "blue" => Ok(0x60a5fa),
        "violet" => Ok(0xa78bfa),
        value => color(value),
    }
}
fn color(value: &str) -> io::Result<u32> {
    parse_annotation_color(value).map_err(invalid)?.ok_or_else(|| unsupported("unsupported annotation color"))
}
fn timestamp(record: &Value, key: &str, format: ReaderFormat, fallback: u64) -> io::Result<u64> {
    match record.get(key) {
        None | Some(Value::Null) => Ok(fallback),
        Some(Value::String(value)) if format == ReaderFormat::Foliate && value.is_empty() => Ok(fallback),
        Some(Value::String(value)) if format == ReaderFormat::Foliate => DateTime::parse_from_rfc3339(value).map_err(invalid)?.timestamp_millis().try_into().map_err(invalid),
        Some(value) if format == ReaderFormat::Readest => value.as_u64().ok_or_else(|| invalid("invalid annotation timestamp")),
        _ => Err(invalid("invalid annotation timestamp")),
    }
}

/// Decode one live note. Callers own envelope checks, identity reconciliation,
/// deleted-note filtering, and the decision whether a new note should be merged.
pub fn decode_annotation(format: ReaderFormat, record: &Value, book: BookId, id: String) -> io::Result<Annotation> {
    if !record.is_object() {
        return Err(invalid("invalid annotation object"));
    }
    let foliate = format == ReaderFormat::Foliate;
    if !foliate && (record.get("type").and_then(Value::as_str).is_some_and(|kind| kind != "annotation") || record["global"].as_bool() == Some(true)) {
        return Err(unsupported("unsupported Readest annotation type"));
    }
    let (style, color) = if foliate {
        match record["color"].as_str().unwrap_or("yellow") {
            "underline" => (AnnotationStyle::Underline, 0xff0000),
            "squiggly" => (AnnotationStyle::Squiggly, 0xff0000),
            "strikethrough" => (AnnotationStyle::Strikethrough, 0xff0000),
            value => (AnnotationStyle::Highlight, color(value)?),
        }
    } else {
        let style = match record["style"].as_str().unwrap_or("highlight") {
            "highlight" => AnnotationStyle::Highlight,
            "underline" => AnnotationStyle::Underline,
            "squiggly" => AnnotationStyle::Squiggly,
            _ => return Err(unsupported("unsupported Readest annotation style")),
        };
        (style, readest_color(record["color"].as_str().unwrap_or("yellow"))?)
    };
    let text = |key: &str| -> io::Result<String> {
        match record.get(key) {
            None | Some(Value::Null) => Ok(String::new()),
            Some(Value::String(value)) if value.len() <= 65_536 => Ok(value.clone()),
            _ => Err(invalid("invalid annotation text")),
        }
    };
    let mut value = decode_annotation_identity(format, record, book, id)?;
    value.exact_text = text("text")?;
    value.note = text("note")?;
    value.style = style;
    value.color = color;
    value.modified_at = timestamp(record, if foliate { "modified" } else { "updatedAt" }, format, value.created_at)?;
    if value.modified_at == 0 {
        value.modified_at = value.created_at;
    }
    value.validate().map_err(invalid)?;
    Ok(value)
}

/// Read identity evidence before reconciliation decides whether a note's body
/// is new. Existing local notes keep their body even if the reader edited it.
pub fn decode_annotation_identity(format: ReaderFormat, record: &Value, book: BookId, id: String) -> io::Result<Annotation> {
    let foliate = format == ReaderFormat::Foliate;
    let cfi = record[if foliate { "value" } else { "cfi" }].as_str().ok_or_else(|| invalid("annotation has no CFI"))?;
    EpubCfi::parse(cfi).map_err(invalid)?;
    let created_at = timestamp(record, if foliate { "created" } else { "createdAt" }, format, 0)?;
    Ok(Annotation {
        id,
        content_hash: book,
        anchor: AnnotationAnchor::epub_publication_cfi(cfi),
        exact_text: String::new(),
        note: String::new(),
        style: AnnotationStyle::Highlight,
        color: 0,
        created_at,
        modified_at: created_at,
        toc_ordinal: None,
        progress: None,
        deleted: false,
    })
}
