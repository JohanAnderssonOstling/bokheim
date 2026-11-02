//! Read structured destinations before replacement. Unknown envelope fields
//! (including Foliate bookmarks) survive; annotation bodies come from the DB.
use crate::failure::failure;
use book_model::{Annotation, EpubCfi};
use content_address::BookId;
use library_model::AnnotationExportFailureKind;
use library_model::{match_foliate_annotations, AnnotationExportFormat};
use reader_formats::{annotation_id, decode_annotation, decode_annotation_identity, ReaderFormat};
use serde_json::{Map, Value};
use std::{collections::HashSet, io};

pub(crate) fn read_json(bytes: &[u8], expected: &Value, book: BookId, history: &[Annotation], format: AnnotationExportFormat) -> io::Result<(Value, Vec<Annotation>, Vec<String>)> {
    let mut document: Value = serde_json::from_slice(bytes).map_err(|error| failure(AnnotationExportFailureKind::InvalidData, error))?;
    if let Some(identity) = document.get("_bokheim_book_id") {
        if *identity != serde_json::to_value(book).map_err(|error| failure(AnnotationExportFailureKind::InvalidData, error))? {
            return Err(failure(AnnotationExportFailureKind::Identity, "existing annotation export belongs to another book"));
        }
    } else if format == AnnotationExportFormat::Foliate {
        let identifier = expected.pointer("/metadata/identifier").and_then(Value::as_str);
        if identifier.is_none() || identifier != document.pointer("/metadata/identifier").and_then(Value::as_str) {
            return Err(failure(AnnotationExportFailureKind::Identity, "existing Foliate file has a different or missing publication identifier"));
        }
    } else if document.pointer("/book/title") != expected.pointer("/book/title") || document.pointer("/book/author") != expected.pointer("/book/author") {
        return Err(failure(AnnotationExportFailureKind::Identity, "existing Readest file does not match this book"));
    }
    if format == AnnotationExportFormat::Readest && (document["$format"] != "readest-annotations" || document["version"] != 1) {
        return Err(failure(AnnotationExportFailureKind::Unsupported, "unsupported Readest annotation export"));
    }
    let identifier = expected.pointer("/metadata/identifier").and_then(Value::as_str).or_else(|| document.pointer("/metadata/identifier").and_then(Value::as_str)).unwrap_or_default().to_owned();
    let readest_hash = document.pointer("/book/hash").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| book.to_string());
    let exported_ids = match document.get("_bokheim_annotation_ids") {
        Some(Value::Object(ids)) => ids.clone(),
        Some(_) => return Err(failure(AnnotationExportFailureKind::InvalidData, "invalid annotation identity map")),
        None => Map::new(),
    };
    let foliate_notes = if format == AnnotationExportFormat::Foliate { resolve_foliate_annotations(&document, book, history, &exported_ids)? } else { Vec::new() };
    let records = document.get_mut("annotations").and_then(Value::as_array_mut).ok_or_else(|| failure(AnnotationExportFailureKind::InvalidData, "annotation export has no annotations array"))?;
    if records.len() > 16_384 {
        return Err(failure(AnnotationExportFailureKind::LimitExceeded, "annotation export contains too many annotations"));
    }
    let mut incoming = Vec::new();
    let mut identities = Vec::with_capacity(records.len());
    let mut seen = HashSet::new();
    for (record_index, record) in records.iter_mut().enumerate() {
        validate_legacy_identity(record, book)?;
        if format == AnnotationExportFormat::Readest {
            if record.get("deletedAt").is_some_and(|value| !value.is_null()) {
                identities.push(String::new());
                continue;
            }
            if record.get("type").and_then(Value::as_str).is_some_and(|kind| kind != "annotation") || record["global"].as_bool() == Some(true) {
                return Err(failure(AnnotationExportFailureKind::Unsupported, "unsupported Readest annotation type"));
            }
        }
        let cfi_key = if format == AnnotationExportFormat::Foliate { "value" } else { "cfi" };
        let cfi = record.get(cfi_key).and_then(Value::as_str).ok_or_else(|| failure(AnnotationExportFailureKind::InvalidData, "annotation has no CFI"))?.to_owned();
        EpubCfi::parse(&cfi).map_err(|error| failure(AnnotationExportFailureKind::InvalidData, error))?;
        // Keep the same namespaces as reader-library import for foreign notes.
        let imported_id = if format == AnnotationExportFormat::Foliate {
            annotation_id(ReaderFormat::Foliate, &identifier, book, &cfi)
        } else {
            let source = record.get("id").and_then(Value::as_str).filter(|id| !id.is_empty()).ok_or_else(|| failure(AnnotationExportFailureKind::InvalidData, "Readest annotation has no ID"))?;
            annotation_id(ReaderFormat::Readest, &readest_hash, book, source)
        };
        let known = history.iter().find(|annotation| annotation.id == imported_id || (format == AnnotationExportFormat::Readest && record["id"].as_str() == Some(&annotation.id)));
        let shared_foliate =
            if known.is_none() && record_identity(record, Some(&exported_ids)).is_none() && format == AnnotationExportFormat::Readest && record["id"].as_str() == Some(crate::reader_profiles::foliate_note_id(&readest_hash, &cfi).as_str()) {
                let mut matching = history.iter().filter(|annotation| annotation.anchor.epub_cfi_value() == Some(cfi.as_str()));
                let first = matching.next();
                if matching.next().is_some() {
                    return Err(failure(AnnotationExportFailureKind::Identity, "Readest's Foliate annotation matches several library annotations"));
                }
                first
            } else {
                None
            };
        let id =
            foliate_notes.get(record_index).map(|note| note.id.clone()).or_else(|| record_identity(record, Some(&exported_ids)).map(str::to_owned)).or_else(|| known.or(shared_foliate).map(|value| value.id.clone())).unwrap_or(imported_id);
        if !seen.insert(id.clone()) {
            return Err(failure(AnnotationExportFailureKind::Identity, "annotations have ambiguous duplicate identities; file left unchanged"));
        }
        identities.push(id.clone());
        let object = record.as_object_mut().ok_or_else(|| failure(AnnotationExportFailureKind::InvalidData, "invalid annotation object"))?;
        // Read old IDs for compatibility, then drop the parsed history before
        // retaining reader fields. It must not enter the next export.
        object.remove("_bokheim");
        object.remove("_bokheim_id");
        if history.iter().any(|annotation| annotation.id == id) {
            continue;
        }
        let format = if format == AnnotationExportFormat::Foliate { ReaderFormat::Foliate } else { ReaderFormat::Readest };
        let value = decode_annotation(format, record, book, id).map_err(note_failure)?;
        incoming.push(value);
    }
    Ok((document, incoming, identities))
}

/// Stable IDs only. Older exports stored the ID inside a complete model.
fn record_identity<'a>(record: &'a Value, identities: Option<&'a Map<String, Value>>) -> Option<&'a str> {
    record
        .get("_bokheim_id")
        .and_then(Value::as_str)
        .or_else(|| record.pointer("/_bokheim/model/id").and_then(Value::as_str))
        .or_else(|| identities.and_then(|ids| record["value"].as_str().or_else(|| record["id"].as_str()).and_then(|key| ids.get(key))).and_then(Value::as_str))
}

fn validate_legacy_identity(record: &Value, book: BookId) -> io::Result<()> {
    if let Some(owner) = record.pointer("/_bokheim/model/content_hash") {
        if *owner != serde_json::to_value(book).map_err(|error| failure(AnnotationExportFailureKind::InvalidData, error))? {
            return Err(failure(AnnotationExportFailureKind::Identity, "annotation belongs to another book"));
        }
    }
    if let Some(id) = record.get("_bokheim_id").or_else(|| record.pointer("/_bokheim/model/id")) {
        if id.as_str().is_none_or(str::is_empty) {
            return Err(failure(AnnotationExportFailureKind::InvalidData, "invalid annotation identity"));
        }
    }
    Ok(())
}

fn note_failure(error: io::Error) -> io::Error {
    let kind = if error.kind() == io::ErrorKind::Unsupported { AnnotationExportFailureKind::Unsupported } else { AnnotationExportFailureKind::InvalidData };
    failure(kind, error)
}

/// Foliate can strip custom IDs when moving a highlight. Resolve an unchanged
/// CFI or a unique, nonzero creation time against current rows and tombstones.
fn resolve_foliate_annotations(document: &Value, book: BookId, history: &[Annotation], exported_ids: &Map<String, Value>) -> io::Result<Vec<Annotation>> {
    let identifier = document.pointer("/metadata/identifier").and_then(Value::as_str).unwrap_or_default();
    let records = document.get("annotations").and_then(Value::as_array).ok_or_else(|| failure(AnnotationExportFailureKind::InvalidData, "annotation export has no annotations array"))?;
    if records.len() > 16_384 {
        return Err(failure(AnnotationExportFailureKind::LimitExceeded, "annotation export contains too many annotations"));
    }
    let mut incoming = records
        .iter()
        .map(|record| {
            validate_legacy_identity(record, book)?;
            let cfi = record["value"].as_str().ok_or_else(|| failure(AnnotationExportFailureKind::InvalidData, "annotation has no CFI"))?;
            EpubCfi::parse(cfi).map_err(|error| failure(AnnotationExportFailureKind::InvalidData, error))?;
            let id = record_identity(record, Some(exported_ids)).map(str::to_owned).unwrap_or_else(|| annotation_id(ReaderFormat::Foliate, identifier, book, cfi));
            decode_annotation_identity(ReaderFormat::Foliate, record, book, id).map_err(note_failure)
        })
        .collect::<io::Result<Vec<_>>>()?;
    let matches = match_foliate_annotations(&incoming, history).map_err(|error| failure(AnnotationExportFailureKind::Identity, error))?;
    for (annotation, matched) in incoming.iter_mut().zip(matches) {
        if let Some(index) = matched {
            annotation.id = history[index].id.clone();
        }
    }
    Ok(incoming)
}

#[cfg(test)]
mod color_tests {
    use super::*;
    use crate::failure::failure_details;

    #[test]
    fn numeric_color_errors_and_unsupported_spellings_keep_their_failure_categories() {
        for value in ["#xyzxyz", "rgb(256,0,0)"] {
            assert_eq!(failure_details(&reader_formats::readest_color(value).map_err(note_failure).unwrap_err()).kind, AnnotationExportFailureKind::InvalidData);
        }
        for value in ["rgba(1,2,3,0.5)", "unknown"] {
            assert_eq!(failure_details(&reader_formats::readest_color(value).map_err(note_failure).unwrap_err()).kind, AnnotationExportFailureKind::Unsupported);
        }
    }
}

/// Foliate constructs GObjects directly from annotation records, so reader
/// extensions must stay outside those records. Preserve opaque fields at the
/// root by annotation ID; they are not annotation history or merge baselines.
fn foliate_field(key: &str) -> bool {
    matches!(key, "value" | "color" | "text" | "note" | "created" | "modified")
}

pub(crate) fn preserve_envelope(mut previous: Value, bytes: &[u8], identities: &[String]) -> io::Result<Vec<u8>> {
    let generated: Value = serde_json::from_slice(bytes).map_err(|error| failure(AnnotationExportFailureKind::InvalidData, error))?;
    let foliate = generated.get("_bokheim_annotation_ids").is_some();
    let generated_ids = generated.get("_bokheim_annotation_ids").and_then(Value::as_object);
    let old_ids = previous.get("_bokheim_annotation_ids").and_then(Value::as_object).cloned();
    let old_fields = previous.get("_bokheim_annotation_fields").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut preserved_fields = Map::new();
    let old_records = previous.get("annotations").and_then(Value::as_array).cloned().unwrap_or_default();
    for (key, mut value) in generated.as_object().expect("generated export object").clone() {
        if key == "annotations" {
            for record in value.as_array_mut().expect("generated annotations") {
                let id = record_identity(record, generated_ids).or_else(|| record["id"].as_str()).map(str::to_owned);
                let mut extra = id.as_ref().and_then(|id| old_fields.get(id)).and_then(Value::as_object).cloned().unwrap_or_default();
                if let Some((_, old)) = old_records.iter().enumerate().find(|(index, old)| {
                    identities.get(*index).map(String::as_str).is_some_and(|old_id| id.as_deref() == Some(old_id))
                        || (record_identity(old, old_ids.as_ref()).is_some() && record_identity(old, old_ids.as_ref()) == id.as_deref())
                        || (old["value"].as_str().is_some() && old["value"] == record["value"])
                        || (old["id"].as_str().is_some() && old["id"] == record["id"])
                }) {
                    for (key, value) in old.as_object().into_iter().flatten() {
                        if matches!(key.as_str(), "_bokheim" | "_bokheim_id") {
                            continue;
                        }
                        if foliate && !foliate_field(key) {
                            extra.insert(key.clone(), value.clone());
                        } else if record.get(key).is_none() {
                            record[key] = value.clone();
                        }
                    }
                }
                if foliate {
                    extra.retain(|key, _| !foliate_field(key) && !matches!(key.as_str(), "_bokheim" | "_bokheim_id"));
                    if let Some(id) = id.filter(|_| !extra.is_empty()) {
                        preserved_fields.insert(id, Value::Object(extra));
                    }
                }
            }
        }
        if key == "bookmarks" && previous.get(&key).is_some() {
            continue;
        }
        if matches!(key.as_str(), "metadata" | "book") {
            if let (Some(old), Some(new)) = (previous.get_mut(&key).and_then(Value::as_object_mut), value.as_object()) {
                for (key, value) in new {
                    if key == "identifier" && value.is_null() && old.get(key).is_some() {
                        continue;
                    }
                    old.insert(key.clone(), value.clone());
                }
                continue;
            }
        }
        previous[&key] = value;
    }
    if foliate {
        if preserved_fields.is_empty() {
            previous.as_object_mut().expect("export object").remove("_bokheim_annotation_fields");
        } else {
            previous["_bokheim_annotation_fields"] = Value::Object(preserved_fields);
        }
    }
    serde_json::to_vec_pretty(&previous).map_err(|error| failure(AnnotationExportFailureKind::InvalidData, error))
}
