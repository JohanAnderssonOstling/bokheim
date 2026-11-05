//! Preserve external annotations and reader fields when replacing an export.
use crate::failure::failure;
use book_model::Annotation;
use content_address::BookId;
use library_model::{AnnotationExportFailureKind, AnnotationExportFormat};
use serde_json::{Map, Value};
use std::{collections::HashSet, io, mem};

/// An empty identity means the record belongs to the destination, not Bokheim.
pub(crate) fn read_json(bytes: &[u8], expected: &Value, book: BookId, history: &[Annotation], format: AnnotationExportFormat) -> io::Result<(Value, Vec<String>)> {
    let document: Value = serde_json::from_slice(bytes).map_err(invalid)?;
    let exported_book = document.get("_bokheim_book_id");
    if let Some(identity) = exported_book {
        if *identity != serde_json::to_value(book).map_err(invalid)? {
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
    let exported_ids = match document.get("_bokheim_annotation_ids") {
        Some(Value::Object(ids)) => Some(ids),
        Some(_) => return Err(invalid("invalid annotation identity map")),
        None => None,
    };
    let records = document.get("annotations").and_then(Value::as_array).ok_or_else(|| invalid("annotation export has no annotations array"))?;
    if records.len() > 16_384 {
        return Err(failure(AnnotationExportFailureKind::LimitExceeded, "annotation export contains too many annotations"));
    }
    let mut identities = Vec::with_capacity(records.len());
    let mut seen = HashSet::new();
    for record in records {
        // Ownership is explicit. A matching passage or creation time does not
        // authorize changing an annotation created in another reader.
        let id = record_identity(record, exported_ids).or_else(|| {
            // Older portable Readest exports used model IDs directly.
            (format == AnnotationExportFormat::Readest && exported_book.is_some()).then(|| record["id"].as_str()).flatten()
        });
        let id = id.filter(|id| history.iter().any(|annotation| annotation.id == *id)).unwrap_or_default();
        if !id.is_empty() && !seen.insert(id) {
            return Err(failure(AnnotationExportFailureKind::Identity, "annotations have ambiguous duplicate identities; file left unchanged"));
        }
        identities.push(id.to_owned());
    }
    Ok((document, identities))
}

fn invalid(error: impl std::fmt::Display) -> io::Error {
    failure(AnnotationExportFailureKind::InvalidData, error)
}

fn record_identity<'a>(record: &'a Value, identities: Option<&'a Map<String, Value>>) -> Option<&'a str> {
    record
        .get("_bokheim_id")
        .and_then(Value::as_str)
        .or_else(|| record.pointer("/_bokheim/model/id").and_then(Value::as_str))
        .or_else(|| identities.and_then(|ids| record["value"].as_str().or_else(|| record["id"].as_str()).and_then(|key| ids.get(key))).and_then(Value::as_str))
}

// Foliate passes annotation records directly to GObject. Keep extra fields on
// owned records in its ignored root-level map instead.
fn foliate_field(key: &str) -> bool {
    matches!(key, "value" | "color" | "text" | "note" | "created" | "modified")
}

pub(crate) fn preserve_envelope(mut previous: Value, bytes: &[u8], identities: &[String]) -> io::Result<Vec<u8>> {
    let mut generated: Value = serde_json::from_slice(bytes).map_err(invalid)?;
    let foliate = generated.get("metadata").is_some();
    let generated_ids = generated.get("_bokheim_annotation_ids").and_then(Value::as_object).cloned();
    let old_fields = previous.get("_bokheim_annotation_fields").and_then(Value::as_object).cloned().unwrap_or_default();
    let old_records = previous.get_mut("annotations").and_then(Value::as_array_mut).map(mem::take).unwrap_or_default();
    let mut preserved_fields = Map::new();
    let records = generated["annotations"].as_array_mut().expect("generated annotations");
    for record in records.iter_mut() {
        let id = record_identity(record, generated_ids.as_ref()).or_else(|| record["id"].as_str()).map(str::to_owned);
        let mut extra = id.as_ref().and_then(|id| old_fields.get(id)).and_then(Value::as_object).cloned().unwrap_or_default();
        if let Some((old, _)) = old_records.iter().zip(identities).find(|(_, old_id)| !old_id.is_empty() && Some(old_id.as_str()) == id.as_deref()) {
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
    let key = if foliate { "value" } else { "id" };
    for (old, identity) in old_records.into_iter().zip(identities) {
        if !identity.is_empty() {
            continue;
        }
        if old[key].as_str().is_some_and(|value| records.iter().any(|record| record[key].as_str() == Some(value))) {
            return Err(failure(AnnotationExportFailureKind::Identity, "an external annotation uses the same location or ID; destination left unchanged"));
        }
        records.push(old);
    }
    for (key, value) in mem::take(generated.as_object_mut().expect("generated export object")) {
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
    serde_json::to_vec_pretty(&previous).map_err(invalid)
}
