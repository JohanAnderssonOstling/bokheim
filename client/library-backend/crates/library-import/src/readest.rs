//! Readest's JSON describes source files; their metadata and reader state are
//! admitted atomically through the verified transfer path. Moves share the
//! native claimed-source workflow with ordinary file imports.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use book_model::{Annotation, BookFormat, EpubCfi, PUBLICATION_CFI_PREFIX, ReadingPosition};
use client_runtime::BackendError;
use filesystem_scanner::ingestion::IngestedFile;
use library_database::transfer::BookTransferState;
use library_replica::{LibraryStateEntry, ReadingPositionState, filenames};
use reader_formats::{ReaderFormat, annotation_id, decode_annotation};
use serde::Deserialize;
use serde_json::Value;
use sync_common::{BookId, DirId};

use crate::LibraryImport;
use crate::reader_transfer::{ReaderMetadata, ReaderPreparation, import_reader_book, reading_progress};

mod library;
pub use library::{ReadestBookCounts, ReadestGroup, ReadestImportOptions, ReadestImportProgress, ReadestImportReport, ReadestLibrary, ReadestLibrarySummary};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadestBook {
    pub hash: String,
    pub format: String,
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub primary_language: String,
    #[serde(default)]
    pub file_path: Option<PathBuf>,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub reading_status: Option<ReadestReadingStatus>,
    #[serde(default)]
    pub group_name: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReadestReadingStatus {
    Unread,
    Reading,
    Finished,
    Abandoned,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadestConfig {
    pub location: Option<String>,
    pub progress: Option<[f64; 2]>,
    #[serde(default)]
    pub booknotes: Vec<Value>,
    #[serde(default, rename = "_bokheim_annotation_ids")]
    pub annotation_ids: BTreeMap<String, String>,
}

pub use crate::reader_import::ImportMode;

pub use crate::reader_transfer::ReaderBookResult as ReadestImportResult;

/// The absolute reference takes precedence; a managed book can still be found
/// under Books/<hash> when an exported reference belongs to another device.
pub fn resolve_book_path(books_directory: &Path, book: &ReadestBook) -> Result<PathBuf, BackendError> {
    if let Some(path) = &book.file_path {
        let path = if path.is_absolute() { path.clone() } else { books_directory.join(path) };
        if path.is_file() {
            return Ok(path);
        }
    }
    let hash = filenames::validate_component(&book.hash)?;
    let format = readest_format(&book.format)?;
    let directory = books_directory.join(hash);
    let candidates =
        fs::read_dir(directory)?.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.is_file() && path.file_name().and_then(|s| s.to_str()).and_then(BookFormat::from_file_name) == Some(format)).collect::<Vec<_>>();
    match candidates.as_slice() {
        [path] => Ok(path.clone()),
        [] => Err(BackendError::message("Readest book file is not available locally")),
        _ => Err(BackendError::message("Readest managed folder contains several possible book files")),
    }
}

fn readest_format(value: &str) -> Result<BookFormat, BackendError> {
    match value {
        "EPUB" => Ok(BookFormat::Epub),
        "PDF" => Ok(BookFormat::Pdf),
        _ => Err(BackendError::message("Readest import currently supports EPUB and PDF books")),
    }
}

pub(crate) async fn import_book_in_batch(
    importer: &LibraryImport, books_directory: &Path, parent: DirId, book: &ReadestBook, config: &ReadestConfig, mode: ImportMode, batch: Option<&mut crate::reader_batch::ReaderImportBatch>,
) -> Result<ReadestImportResult, BackendError> {
    let source = resolve_book_path(books_directory, book)?;
    let format = readest_format(&book.format)?;
    let prepare = ReaderPreparation::Readest { book: book.clone(), config: config.clone() };
    let mut result = import_reader_book(importer, parent, &source, format, mode, batch, prepare).await?;
    result.skipped_pdf_annotations = if format == BookFormat::Pdf { config.booknotes.len() } else { 0 };
    Ok(result)
}

pub(crate) fn prepare_state(book: &ReadestBook, config: &ReadestConfig, ingested: &IngestedFile, warnings: &mut Vec<String>) -> Result<BookTransferState, BackendError> {
    let mut state = ReaderMetadata { title: &book.title, author: &book.author, language: &book.primary_language, tags: &book.tags, metadata: &book.metadata, source: "Readest" }.transfer_state(ingested, warnings)?;
    state.annotations = epub_annotations(book, config, ingested.book_id, ingested.format)?;
    state.reading = reading_state(book, config, ingested.book_id, ingested.format)?;
    state.chapter_read = match book.reading_status {
        Some(ReadestReadingStatus::Finished | ReadestReadingStatus::Unread) => {
            let mut value = book_model::ChapterReadStatus::default();
            value.mark_all(&ingested.inspection.toc, book.reading_status == Some(ReadestReadingStatus::Finished));
            Some(LibraryStateEntry::ChapterReadStatus { book_id: ingested.book_id, value })
        }
        Some(ReadestReadingStatus::Abandoned) => {
            warnings.push("Readest abandoned status has no Bokheim equivalent; reading progress is retained".into());
            None
        }
        _ => None,
    };
    if book.created_at > i64::MAX as u64 {
        return Err(BackendError::message("Readest creation date exceeds the database range"));
    }
    state.addition.facts.added_at = book.created_at;
    Ok(state)
}

fn reading_state(book: &ReadestBook, config: &ReadestConfig, hash: BookId, format: BookFormat) -> Result<Option<LibraryStateEntry>, BackendError> {
    let location = match config.location.as_deref().filter(|location| !location.is_empty()) {
        None if book.reading_status == Some(ReadestReadingStatus::Finished) => ReadingPosition::start(format),
        None => return Ok(None),
        Some(location) if format == BookFormat::Pdf => {
            let prefix = location.split('!').next().unwrap_or(location).trim_end_matches(')');
            let step = prefix.strip_prefix("epubcfi(/6/").and_then(|value| value.split('[').next()).and_then(|value| value.parse::<u32>().ok()).filter(|step| *step >= 2 && step % 2 == 0).ok_or("invalid Readest PDF location")?;
            ReadingPosition::pdf_page(step / 2 - 1)
        }
        Some(location) => {
            EpubCfi::parse(location).map_err(BackendError::operation)?;
            ReadingPosition::parse(&format!("{PUBLICATION_CFI_PREFIX}{location}")).map_err(BackendError::operation)?
        }
    };
    let progress = match book.reading_status {
        Some(ReadestReadingStatus::Finished) => 100.0,
        Some(ReadestReadingStatus::Unread) => 0.0,
        _ => reading_progress(config.progress, "Readest")?,
    };
    Ok(Some(LibraryStateEntry::ReadingPosition { book_id: hash, value: ReadingPositionState { location, progress } }))
}

fn epub_annotations(book: &ReadestBook, config: &ReadestConfig, hash: BookId, format: BookFormat) -> Result<Vec<Annotation>, BackendError> {
    // Deliberately never interpret Readest's PDF text-layer CFIs as EPUB anchors.
    if format == BookFormat::Pdf {
        return Ok(Vec::new());
    }
    let mut annotations = Vec::new();
    for note in &config.booknotes {
        if !note["deletedAt"].is_null() {
            continue;
        }
        if note["type"].as_str() != Some("annotation") {
            return Err(BackendError::message("this Readest EPUB note type cannot yet be imported; source retained"));
        }
        if note["global"].as_bool() == Some(true) {
            return Err(BackendError::message("global Readest highlights cannot yet be imported; source retained"));
        }
        let id = note["id"].as_str().ok_or("Readest annotation has no id")?;
        note["createdAt"].as_u64().ok_or("invalid Readest annotation creation date")?;
        let exported_id = note.get("_bokheim_id").map(|value| value.as_str().filter(|value| !value.is_empty()).ok_or("invalid exported Readest annotation identity")).transpose()?;
        let id = exported_id.map(str::to_owned).or_else(|| config.annotation_ids.get(id).cloned()).unwrap_or_else(|| annotation_id(ReaderFormat::Readest, &book.hash, hash, id));
        let annotation = decode_annotation(ReaderFormat::Readest, note, hash, id).map_err(BackendError::operation)?;
        annotations.push(annotation);
    }
    Ok(annotations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn book(extra: Value) -> ReadestBook {
        let mut data = json!({"hash":"example", "format":"EPUB", "title":"Main title: Subtitle"});
        data.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        serde_json::from_value(data).unwrap()
    }

    #[test]
    fn finished_status_survives_without_a_saved_location() {
        let source = book(json!({"readingStatus":"finished"}));
        let hash = BookId::from(blake3::hash(b"fixture"));
        let state = reading_state(&source, &ReadestConfig::default(), hash, BookFormat::Epub).unwrap().unwrap();
        let LibraryStateEntry::ReadingPosition { value, .. } = state else { panic!("expected reading state") };
        assert_eq!(value.progress, 100.0);
    }

    #[test]
    fn explicit_unread_status_retains_location_and_resets_progress() {
        let source = book(json!({"readingStatus":"unread"}));
        let config = ReadestConfig { location: Some("epubcfi(/6/4)".into()), progress: Some([99.0, 100.0]), ..Default::default() };
        let hash = BookId::from(blake3::hash(b"fixture"));
        let state = reading_state(&source, &config, hash, BookFormat::Epub).unwrap().unwrap();
        let LibraryStateEntry::ReadingPosition { value, .. } = state else { panic!("expected reading state") };
        assert_eq!(value.progress, 0.0);
        assert_eq!(value.location.epub_cfi_value(), Some("epubcfi(/6/4)"));
    }

    #[test]
    fn explicit_status_overrides_progress_without_bypassing_location_validation() {
        let hash = BookId::from_bytes([1; 32]);
        for (status, expected) in [("finished", 100.0), ("unread", 0.0)] {
            let source = book(json!({"readingStatus":status}));
            let mut config = ReadestConfig { location: Some("epubcfi(/6/4)".into()), progress: Some([f64::NAN, 0.0]), ..Default::default() };
            let Some(LibraryStateEntry::ReadingPosition { value, .. }) = reading_state(&source, &config, hash, BookFormat::Epub).unwrap() else { panic!("expected reading state") };
            assert_eq!(value.progress, expected);
            config.location = Some("invalid CFI".into());
            assert!(reading_state(&source, &config, hash, BookFormat::Epub).is_err());
        }
    }
    #[test]
    fn installed_annotations_keep_bokheim_ids_on_reader_import() {
        let source = book(json!({"hash":"reader-hash"}));
        let config: ReadestConfig = serde_json::from_value(json!({
            "booknotes":[{"id":"reader-note","type":"annotation","cfi":"epubcfi(/6/2!/4/2/1:0)","text":"Quote","note":"Note","createdAt":1,"updatedAt":2}],
            "_bokheim_annotation_ids":{"reader-note":"bokheim-note"}
        }))
        .unwrap();
        let annotations = epub_annotations(&source, &config, BookId::from_bytes([3; 32]), BookFormat::Epub).unwrap();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0].id, "bokheim-note");
        let mut rewritten = config;
        rewritten.annotation_ids.clear();
        rewritten.booknotes[0]["_bokheim_id"] = "bokheim-note".into();
        assert_eq!(epub_annotations(&source, &rewritten, BookId::from_bytes([3; 32]), BookFormat::Epub).unwrap()[0].id, "bokheim-note");
    }
}
