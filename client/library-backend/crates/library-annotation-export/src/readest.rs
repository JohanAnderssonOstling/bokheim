//! Readest's versioned annotation import contract, rather than its device config.
//! https://github.com/readest/readest/blob/main/apps/readest-app/src/services/annotation/providers/readest.ts
use crate::failure::failure;
use book_model::{AnnotationStyle, BookFormat, EpubCfi};
use library_model::AnnotationExportBook;
use library_model::AnnotationExportFailureKind;
use serde_json::json;
use std::io;

pub fn readest(book: &AnnotationExportBook, exported_at: u64) -> io::Result<Vec<u8>> {
    if book.format != BookFormat::Epub {
        return Err(failure(AnnotationExportFailureKind::Unsupported, "Readest exports require an EPUB book"));
    }
    let annotations = book
        .annotations
        .iter()
        .filter(|annotation| !annotation.deleted)
        .map(|annotation| {
            if !annotation.anchor.is_epub_publication_cfi() {
                return Err(failure(AnnotationExportFailureKind::Unavailable, "legacy EPUB annotations need their local publication to resolve full CFIs"));
            }
            let cfi = annotation.anchor.epub_cfi_value().ok_or_else(|| failure(AnnotationExportFailureKind::InvalidData, "EPUB annotation has no CFI"))?;
            EpubCfi::parse(cfi).map_err(|error| failure(AnnotationExportFailureKind::InvalidData, error))?;
            let style = match annotation.style {
                AnnotationStyle::Highlight => "highlight",
                AnnotationStyle::Underline | AnnotationStyle::Strikethrough => "underline",
                AnnotationStyle::Squiggly => "squiggly",
            };
            let record = json!({
                "id": annotation.id, "type": "annotation", "cfi": cfi,
                "text": annotation.exact_text, "note": annotation.note,
                "style": style, "color": format!("#{:06x}", annotation.color),
                "createdAt": annotation.created_at, "updatedAt": annotation.modified_at,
            });
            Ok(record)
        })
        .collect::<io::Result<Vec<_>>>()?;
    // Readest hashes are partial MD5s, not our book IDs or content hashes. They
    // are optional in this contract: the user imports into an already open book.
    serde_json::to_vec_pretty(&json!({
        "$format": "readest-annotations", "version": 1, "exportedAt": exported_at,
        "_bokheim_book_id": book.book_id,
        "book": {"title": book.metadata.title, "author": book.metadata.authors().map(|author| author.name()).collect::<Vec<_>>().join(", "), "format": "EPUB"},
        "annotations": annotations,
    }))
    .map_err(io::Error::other)
}
