//! Foliate's JSON import contract: metadata plus annotations with publication CFIs.
//! See https://github.com/johnfactotum/foliate/blob/gtk4/src/annotations.js
use crate::failure::failure;
use crate::markdown::timestamp;
use book_model::{AnnotationStyle, BookFormat, EpubCfi};
use library_model::AnnotationExportBook;
use library_model::AnnotationExportFailureKind;
use serde_json::{json, Map, Value};
use std::io;

pub fn foliate(book: &AnnotationExportBook, identifier: Option<&str>) -> io::Result<Vec<u8>> {
    if book.format != BookFormat::Epub {
        return Err(failure(AnnotationExportFailureKind::Unsupported, "Foliate exports require an EPUB book"));
    }
    let annotations = book
        .annotations
        .iter()
        .filter(|a| !a.deleted)
        .map(|annotation| {
            if !annotation.anchor.is_epub_publication_cfi() {
                return Err(failure(AnnotationExportFailureKind::InvalidData, "EPUB annotation has no publication CFI"));
            }
            let value = annotation.anchor.epub_cfi_value().ok_or_else(|| failure(AnnotationExportFailureKind::InvalidData, "EPUB annotation has no CFI"))?;
            EpubCfi::parse(value).map_err(|error| failure(AnnotationExportFailureKind::InvalidData, error))?;
            let color = match annotation.style {
                AnnotationStyle::Highlight => format!("#{:06x}", annotation.color),
                AnnotationStyle::Underline => "underline".into(),
                AnnotationStyle::Squiggly => "squiggly".into(),
                AnnotationStyle::Strikethrough => "strikethrough".into(),
            };
            let record = json!({ "value":value,"color":color,"text":annotation.exact_text,"note":annotation.note,"created":timestamp(annotation.created_at)?,"modified":timestamp(annotation.modified_at)? });
            Ok(record)
        })
        .collect::<io::Result<Vec<_>>>()?;
    // Foliate passes each annotation straight to GObject: only its six native
    // fields are allowed. Keep our identities in an ignored root-level map.
    let mut identities = Map::new();
    for (record, annotation) in annotations.iter().zip(book.annotations.iter().filter(|annotation| !annotation.deleted)) {
        let cfi = record["value"].as_str().expect("validated CFI");
        if identities.insert(cfi.to_owned(), Value::String(annotation.id.clone())).is_some() {
            return Err(failure(AnnotationExportFailureKind::Unsupported, "Foliate supports only one annotation at each CFI"));
        }
    }
    serde_json::to_vec_pretty(
        &json!({"metadata": {"title":book.metadata.title,"creator":book.metadata.authors().map(|a|a.name()).collect::<Vec<_>>().join(", "),"identifier":identifier},"_bokheim_book_id":book.book_id,"_bokheim_annotation_ids":identities,"annotations":annotations,"bookmarks":[]}),
    )
    .map_err(io::Error::other)
}
