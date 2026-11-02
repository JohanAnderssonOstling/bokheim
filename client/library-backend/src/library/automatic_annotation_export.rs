//! Device-local, coalesced exports. Receipts survive restarts and only advance
//! after an atomic write succeeds. Neither settings nor receipts enter sync.
use super::LibrarySession;
use crate::BackendError;
use client_platform_runtime::executor::run_blocking;
use futures_util::FutureExt;
use library_annotation_export::{export_prepared, plan_with_existing, validate_export_folder, ExportControl};
use library_model::{
    AnnotationExportBook, AnnotationExportConflict, AnnotationExportDestination, AnnotationExportFormat, AnnotationExportOptions, AnnotationExportOutcome, AnnotationExportReport, AutomaticAnnotationExportStatus, LibraryUpdate,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    fs,
    io::{self, Write},
    panic::AssertUnwindSafe,
    path::{Component, Path},
    time::Duration,
};
mod status;
use library_model::{AnnotationExportFailure, AnnotationExportFailureKind};
use status::ExportState;
use sync_common::BookId;
use tempfile::NamedTempFile;
use tokio::time::{sleep, sleep_until, timeout, Instant};

#[derive(Default)]
pub(crate) struct AutomaticExport {
    configuration: RefCell<Option<Option<AnnotationExportOptions>>>,
    status: RefCell<Option<ExportState>>,
    status_dirty: Cell<bool>,
    generation: Cell<u64>,
    active: RefCell<Option<ExportControl>>,
    manual: RefCell<Vec<Receipt>>,
    receipts_dirty: Cell<bool>,
    initialized: Cell<bool>,
    pending: RefCell<HashSet<BookId>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Receipt {
    destination: AnnotationExportDestination,
    fingerprint: String,
    #[serde(default)]
    last_success_at: Option<u64>,
}

impl LibrarySession {
    /// Manual and automatic exports share existing filename allocations.
    pub(crate) fn annotation_export_options_for_books(&self, books: &[AnnotationExportBook], mut options: AnnotationExportOptions) -> Result<AnnotationExportOptions, BackendError> {
        if options.conflict != AnnotationExportConflict::Replace {
            return Ok(options);
        }
        let receipts = read_json::<Vec<Receipt>>(&self.db.path().with_extension("annotation-export-receipts.json")).map_err(BackendError::operation)?.unwrap_or_default();
        let existing = receipts.iter().chain(self.runtime.automatic_export.manual.borrow().iter()).map(|receipt| receipt.destination.clone()).collect::<Vec<_>>();
        let planned = plan_with_existing(self.id, books, &options, &existing).map_err(BackendError::operation)?;
        options.overrides = planned.items.into_iter().filter(|destination| destination.format != AnnotationExportFormat::Pdf).collect();
        Ok(options)
    }

    pub(crate) fn remember_annotation_export(&self, books: &[AnnotationExportBook], destinations: &[AnnotationExportDestination], report: &AnnotationExportReport, install_readers: bool) -> Result<(), BackendError> {
        let mut state = status::load(self)?;
        let mut manual = self.runtime.automatic_export.manual.borrow_mut();
        for result in &report.results {
            if !matches!(result.outcome, AnnotationExportOutcome::Written) {
                continue;
            }
            if report.results.iter().any(|other| {
                other.destination.book_id == result.destination.book_id
                    && other.destination.directory_id == result.destination.directory_id
                    && other.destination.format == result.destination.format
                    && !matches!(other.outcome, AnnotationExportOutcome::Written)
            }) {
                continue;
            }
            let Some(destination) = destinations.iter().find(|item| item.book_id == result.destination.book_id && item.directory_id == result.destination.directory_id && item.format == result.destination.format) else {
                continue;
            };
            let Some(book) = books.iter().find(|book| book.book_id == destination.book_id) else {
                continue;
            };
            let receipt = refreshed_receipt(self, book, destination.clone(), install_readers)?;
            manual.retain(|old| !same_destination(&old.destination, destination));
            state.success(destination, status::now());
            manual.push(receipt);
        }
        self.events.annotation_exports.notify_one();
        status::publish(self, state)
    }
    pub(crate) fn annotation_export_tracking_failed(&self, error: &BackendError) {
        status::global_error(self, format!("Could not record completed exports: {error}"));
        self.events.annotation_exports.notify_one();
    }
    pub(crate) fn automatic_annotation_export(&self) -> Result<AutomaticAnnotationExportStatus, BackendError> {
        let automatic = &self.runtime.automatic_export;
        if automatic.configuration.borrow().is_none() {
            let options = read_json::<Option<AnnotationExportOptions>>(&self.db.path().with_extension("annotation-export-settings.json")).map_err(BackendError::operation)?.flatten();
            automatic.configuration.replace(Some(options));
        }
        let state = status::load(self)?;
        let outputs = state
            .failures
            .into_iter()
            .map(|entry| {
                let mut output = entry.output;
                if output.destination.format == AnnotationExportFormat::Pdf {
                    output.destination.path = self.assets.native_library_root().join(output.destination.path.to_string_lossy().trim_start_matches('/'));
                }
                output
            })
            .collect();
        Ok(AutomaticAnnotationExportStatus { options: automatic.configuration.borrow().as_ref().cloned().flatten(), error: state.error, outputs, last_success_at: state.last_success_at })
    }

    pub(crate) fn set_automatic_annotation_export(&self, mut options: Option<AnnotationExportOptions>) -> Result<(), BackendError> {
        if let Some(options) = &mut options {
            let uses_folder = options.formats.iter().any(|format| *format == AnnotationExportFormat::Markdown || (!options.install_readers && *format != AnnotationExportFormat::Pdf));
            if options.formats.is_empty() || (uses_folder && (!options.destination.is_absolute() || options.destination.components().any(|part| matches!(part, Component::ParentDir)))) {
                return Err(BackendError::message("select export formats and an absolute export folder for generated documents"));
            }
            options.conflict = AnnotationExportConflict::Replace;
            options.overrides.clear();
            if uses_folder {
                validate_export_folder(self.assets.native_library_root(), &options.destination).map_err(BackendError::operation)?;
            }
        }
        let changed = self.automatic_annotation_export()?.options != options;
        if options.is_some() {
            self.db.begin_annotation_export_tracking()?;
        }
        write_json(&self.db.path().with_extension("annotation-export-settings.json"), &options).map_err(BackendError::operation)?;
        let automatic = &self.runtime.automatic_export;
        if let Some(active) = automatic.active.borrow().as_ref() {
            active.cancel();
        }
        automatic.generation.set(automatic.generation.get().wrapping_add(1));
        automatic.configuration.replace(Some(options));
        self.events.annotation_exports.notify_one();
        if changed {
            automatic.initialized.set(false);
            let mut state = status::load(self)?;
            state.error = None;
            state.failures.clear();
            status::publish(self, state)?;
        }
        self.events.publish_update(LibraryUpdate::AnnotationExportStatusChanged);
        Ok(())
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(io::Error::other),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let mut file = NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    file.write_all(&serde_json::to_vec(value).map_err(io::Error::other)?)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}

/// A PDF export changes its local checksum. Excluding that checksum prevents
/// feedback loops, while including destination paths handles moves/collisions.
fn fingerprint(book: &AnnotationExportBook, format: AnnotationExportFormat, install_readers: bool) -> Result<String, BackendError> {
    let bytes = if format == AnnotationExportFormat::Pdf {
        serde_json::to_vec(&(book.book_id, &book.annotations))
    } else {
        let hashes = (format == AnnotationExportFormat::Markdown).then(|| (book.canonical_hash, book.placements.iter().map(|placement| (placement.directory_id, placement.local_hash)).collect::<Vec<_>>()));
        serde_json::to_vec(&(book.book_id, &book.metadata, &book.toc, &book.annotations, hashes))
    }
    .map_err(BackendError::operation)?;
    let mut digest = blake3::Hasher::new();
    digest.update(&bytes);
    if install_readers && matches!(format, AnnotationExportFormat::Foliate | AnnotationExportFormat::Readest) {
        digest.update(b"installed-reader-profile-v1");
    }
    Ok(digest.finalize().to_hex().to_string())
}

fn same_destination(a: &AnnotationExportDestination, b: &AnnotationExportDestination) -> bool {
    a.book_id == b.book_id && a.directory_id == b.directory_id && a.format == b.format && a.path == b.path
}

fn refreshed_receipt(session: &LibrarySession, original: &AnnotationExportBook, destination: AnnotationExportDestination, install_readers: bool) -> Result<Receipt, BackendError> {
    let mut book = original.clone();
    if destination.format == AnnotationExportFormat::Markdown {
        for placement in &mut book.placements {
            placement.local_hash = session.db.local_content_hash(book.book_id, placement.directory_id)?;
        }
    }
    Ok(Receipt { fingerprint: fingerprint(&book, destination.format, install_readers)?, destination, last_success_at: Some(status::now()) })
}

struct ActivePass<'a>(&'a AutomaticExport);
impl Drop for ActivePass<'_> {
    fn drop(&mut self) {
        self.0.active.replace(None);
    }
}

async fn save_receipts(session: &LibrarySession, receipts: &[Receipt], path: &Path) -> Result<(), BackendError> {
    if !session.runtime.automatic_export.receipts_dirty.get() {
        return Ok(());
    }
    let saved = receipts.to_vec();
    let path = path.to_owned();
    run_blocking(move || write_json(&path, &saved)).await.map_err(BackendError::operation)?.map_err(BackendError::operation)?;
    session.runtime.automatic_export.receipts_dirty.set(false);
    Ok(())
}

async fn pass(session: &LibrarySession, receipts: &mut Vec<Receipt>, receipts_path: &Path) -> Result<bool, BackendError> {
    if session.scanner_running() || !session.runtime.annotation_exports.is_empty() {
        return Ok(true);
    }
    let _export = session.runtime.annotation_export_lock.lock().await;
    save_receipts(session, receipts, receipts_path).await.map_err(|error| error.context("Files exported; saving completion receipts failed"))?;
    let Some(options) = session.automatic_annotation_export()?.options else { return Ok(false) };
    let generation = session.runtime.automatic_export.generation.get();
    let automatic = &session.runtime.automatic_export;
    if !automatic.initialized.get() {
        session.db.begin_annotation_export_tracking()?;
        automatic.pending.borrow_mut().extend(session.db.annotation_export_book_ids()?);
        automatic.initialized.set(true);
    }
    automatic.pending.borrow_mut().extend(session.db.take_annotation_export_changes()?);
    let mut state = status::load(session)?;
    automatic.pending.borrow_mut().extend(state.failures.iter().filter(|entry| entry.due(session, status::now())).map(|entry| entry.output.destination.book_id));
    state.error = None;
    let manual = session.runtime.automatic_export.manual.take();
    if !manual.is_empty() {
        for receipt in manual {
            automatic.pending.borrow_mut().insert(receipt.destination.book_id);
            receipts.retain(|old| !same_destination(&old.destination, &receipt.destination));
            state.success(&receipt.destination, receipt.last_success_at.unwrap_or_else(status::now));
            receipts.push(receipt);
        }
        session.runtime.automatic_export.receipts_dirty.set(true);
        save_receipts(session, receipts, receipts_path).await.map_err(|error| error.context("Files exported; saving completion receipts failed"))?;
    }
    if generation != automatic.generation.get() {
        return Ok(true);
    }
    let mut affected = automatic.pending.borrow().iter().copied().collect::<Vec<_>>();
    affected.sort();
    if affected.is_empty() {
        let retry = !state.failures.is_empty();
        status::publish(session, state)?;
        return Ok(retry);
    }
    let affected_set = affected.iter().copied().collect::<HashSet<_>>();
    let mut books = session.db.annotation_export_books(&affected)?;
    let existing = receipts.iter().map(|receipt| receipt.destination.clone()).chain(state.failures.iter().map(|entry| entry.output.destination.clone())).collect::<Vec<_>>();
    let plan = plan_with_existing(session.id, &books, &options, &existing).map_err(BackendError::operation)?;
    state.failures.retain(|entry| !affected_set.contains(&entry.output.destination.book_id) || plan.items.iter().any(|item| same_destination(item, &entry.output.destination)));
    let mut expected = Vec::new();
    let mut pending = false;
    let now = status::now();
    for destination in &plan.items {
        let book = books.iter().find(|book| book.book_id == destination.book_id).expect("planned book");
        let fingerprint = fingerprint(book, destination.format, options.install_readers)?;
        if receipts.iter().any(|receipt| same_destination(&receipt.destination, destination) && receipt.fingerprint == fingerprint) {
            state.failures.retain(|entry| !same_destination(&entry.output.destination, destination));
            continue;
        }
        pending = true;
        let receipt = Receipt { destination: destination.clone(), fingerprint, last_success_at: None };
        let blocked = state.failures.iter().find(|entry| same_destination(&entry.output.destination, destination));
        if blocked.is_none_or(|entry| entry.ready(session, &receipt, now)) {
            expected.push(receipt);
        }
    }
    // A delayed PDF write refreshes its Markdown so frontmatter follows the
    // resulting local hash. A publication lock never belongs to an open reader.
    let mut writable_pdfs = Vec::new();
    if session.assets.root_available() {
        for receipt in expected.iter().filter(|receipt| receipt.destination.format == AnnotationExportFormat::Pdf) {
            let book = books.iter().find(|book| book.book_id == receipt.destination.book_id).expect("planned book");
            if book.placements.iter().any(|placement| placement.directory_id == receipt.destination.directory_id && placement.downloaded) && session.assets.try_exclusive_lease(&book.book_id)?.is_some() {
                writable_pdfs.push((book.book_id, receipt.destination.directory_id));
            }
        }
    }
    for destination in &plan.items {
        if destination.format == AnnotationExportFormat::Markdown && writable_pdfs.contains(&(destination.book_id, destination.directory_id)) && !expected.iter().any(|receipt| same_destination(&receipt.destination, destination)) {
            let book = books.iter().find(|book| book.book_id == destination.book_id).expect("planned book");
            expected.push(Receipt { destination: destination.clone(), fingerprint: fingerprint(book, destination.format, options.install_readers)?, last_success_at: None });
        }
    }
    if expected.is_empty() {
        let retry = pending || !state.failures.is_empty();
        status::publish(session, state)?;
        automatic.pending.borrow_mut().retain(|book| !affected_set.contains(book));
        return Ok(retry);
    }
    let selected = expected.iter().map(|receipt| receipt.destination.clone()).collect::<Vec<_>>();
    let selected_books = selected.iter().map(|destination| destination.book_id).collect::<HashSet<_>>();
    books.retain(|book| selected_books.contains(&book.book_id));
    let control = ExportControl::default();
    session.runtime.automatic_export.active.replace(Some(control.clone()));
    let _active = ActivePass(&session.runtime.automatic_export);
    let before_import = session.db.sync_outbox_watermark()?;
    let install_readers = options.install_readers;
    let report = export_prepared(&session.db, &session.assets, session.id, options, &mut books, library_model::AnnotationExportPlan { library_id: session.id, items: selected }, control, |_| {}).await.map_err(BackendError::operation)?;
    if session.db.sync_outbox_watermark()? != before_import {
        session.events.sync.wake();
        session.events.publish_update(LibraryUpdate::Annotations);
        session.events.annotation_exports.notify_one();
    }
    let failed =
        report.results.iter().filter(|result| !matches!(result.outcome, AnnotationExportOutcome::Written)).map(|result| (result.destination.book_id, result.destination.directory_id, result.destination.format as u8)).collect::<HashSet<_>>();
    let written =
        report.results.iter().filter(|result| matches!(result.outcome, AnnotationExportOutcome::Written)).map(|result| (result.destination.book_id, result.destination.directory_id, result.destination.format as u8)).collect::<HashSet<_>>();
    let mut changed = false;
    for result in report.results {
        let Some(receipt) =
            expected.iter().find(|receipt| receipt.destination.book_id == result.destination.book_id && receipt.destination.directory_id == result.destination.directory_id && receipt.destination.format == result.destination.format)
        else {
            continue;
        };
        match result.outcome {
            AnnotationExportOutcome::Written => {
                if failed.contains(&(result.destination.book_id, result.destination.directory_id, result.destination.format as u8)) {
                    continue;
                }
                if result.destination.format == AnnotationExportFormat::Pdf {
                    session.events.annotation_exports.notify_one();
                }
                let book = books.iter().find(|book| book.book_id == receipt.destination.book_id).expect("planned book");
                match refreshed_receipt(session, book, receipt.destination.clone(), install_readers) {
                    Ok(refreshed) => {
                        receipts.retain(|old| !same_destination(&old.destination, &receipt.destination));
                        state.success(&receipt.destination, refreshed.last_success_at.unwrap_or(now));
                        receipts.push(refreshed);
                        changed = true;
                    }
                    Err(error) => {
                        let last_success = receipts.iter().find(|old| same_destination(&old.destination, &receipt.destination)).and_then(|receipt| receipt.last_success_at);
                        state.failed(session, receipt, book.metadata.title.clone(), AnnotationExportFailure { kind: AnnotationExportFailureKind::Tracking, file_written: true }, error.to_string(), last_success, status::now());
                    }
                }
            }
            AnnotationExportOutcome::Failed(message) => {
                let mut failure = result.failure.unwrap_or(AnnotationExportFailure { kind: AnnotationExportFailureKind::Io, file_written: false });
                failure.file_written |= written.contains(&(result.destination.book_id, result.destination.directory_id, result.destination.format as u8));
                let book = books.iter().find(|book| book.book_id == receipt.destination.book_id).expect("planned book");
                let last_success = receipts.iter().find(|old| same_destination(&old.destination, &receipt.destination)).and_then(|receipt| receipt.last_success_at);
                state.failed(session, receipt, book.metadata.title.clone(), failure, message, last_success, status::now());
            }
            AnnotationExportOutcome::Skipped => {}
        }
    }
    if changed {
        session.runtime.automatic_export.receipts_dirty.set(true);
    }
    // An options change or explicit retry owns the newer status. Completed
    // writes still retain receipts, but an older pass cannot undo that request.
    if generation == session.runtime.automatic_export.generation.get() {
        status::publish(session, state.clone())?;
    }
    save_receipts(session, receipts, receipts_path).await.map_err(|error| error.context("Files exported; saving completion receipts failed"))?;
    if generation == automatic.generation.get() && !report.cancelled {
        automatic.pending.borrow_mut().retain(|book| !affected_set.contains(book));
    }
    Ok(report.cancelled || !state.failures.is_empty())
}

pub(super) async fn run(session: &LibrarySession) {
    if session.is_opds() {
        return;
    }
    let path = session.db.path().with_extension("annotation-export-receipts.json");
    let read_path = path.clone();
    let mut receipts = match run_blocking(move || read_json::<Vec<Receipt>>(&read_path)).await {
        Ok(Ok(receipts)) => receipts.unwrap_or_default(),
        result => {
            log::warn!("automatic annotation export receipts could not be read; exports will be rebuilt: {result:?}");
            Vec::new()
        }
    };
    let mut global_attempts = 0_u32;
    loop {
        let automatic = &session.runtime.automatic_export;
        let generation = automatic.generation.get();
        let result = AssertUnwindSafe(pass(session, &mut receipts, &path)).catch_unwind().await.unwrap_or_else(|_| Err(BackendError::message("automatic annotation export interrupted; retrying")));
        let retry = match &result {
            Ok(retry) => *retry,
            Err(_) => true,
        };
        if generation == automatic.generation.get() {
            if let Err(error) = &result {
                global_attempts = global_attempts.saturating_add(1);
                log::warn!("automatic annotation export will retry: {error}");
                status::global_error(session, error.to_string());
            } else {
                global_attempts = 0;
            }
        }
        if retry {
            tokio::select! {
                _ = session.events.annotation_exports.notified() => {},
                _ = sleep(Duration::from_millis(if global_attempts == 0 { 30_000 } else { status::backoff(global_attempts) })) => {},
            }
        } else {
            session.events.annotation_exports.notified().await;
        }
        // Two seconds of quiet, capped at ten seconds for continuous editing.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            tokio::select! {
                _ = sleep_until(deadline) => break,
                result = timeout(Duration::from_secs(2), session.events.annotation_exports.notified()) => if result.is_err() { break; },
            }
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
mod reader_lifecycle_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::handle::concurrency_tests::fixture;
    use crate::BookId;
    use book_model::{Annotation, AnnotationAnchor, AnnotationStyle, BookDetails, BookFormat, PdfAnnotationAnchor, PdfAnnotationRect};
    use library_annotation_export::plan;
    use library_database::ImportCommit;
    use library_model::PublishedStagedBook;
    use lopdf::{dictionary, Document, Object};
    use pdf_view_common::{PdfReaderMetadata, PdfStoredPage};
    use std::io::{Read, Seek};
    use sync_common::ROOT_DIR_ID;

    fn note(book: BookId, anchor: AnnotationAnchor) -> Annotation {
        Annotation {
            id: "auto-note".into(),
            content_hash: book,
            anchor,
            exact_text: "Quote".into(),
            style: AnnotationStyle::Highlight,
            color: 0xffcc00,
            note: "First note".into(),
            created_at: 1,
            modified_at: 2,
            toc_ordinal: None,
            progress: None,
            deleted: false,
        }
    }

    fn options(folder: &Path, formats: Vec<AnnotationExportFormat>) -> AnnotationExportOptions {
        AnnotationExportOptions { destination: folder.to_owned(), formats, install_readers: false, conflict: AnnotationExportConflict::Replace, overrides: vec![] }
    }

    fn outbox(session: &LibrarySession) -> i64 {
        rusqlite::Connection::open(session.db.path()).unwrap().query_row("SELECT count(*) FROM sync_outbox", [], |row| row.get(0)).unwrap()
    }

    #[test]
    fn portable_receipts_do_not_skip_the_first_reader_installation() {
        let book = AnnotationExportBook { book_id: BookId::from_bytes([7; 32]), format: book_model::BookFormat::Epub, metadata: Default::default(), toc: vec![], annotations: vec![], canonical_hash: None, placements: vec![] };
        for format in [AnnotationExportFormat::Foliate, AnnotationExportFormat::Readest] {
            assert_ne!(fingerprint(&book, format, false).unwrap(), fingerprint(&book, format, true).unwrap());
        }
        for format in [AnnotationExportFormat::Markdown, AnnotationExportFormat::Pdf] {
            assert_eq!(fingerprint(&book, format, false).unwrap(), fingerprint(&book, format, true).unwrap());
        }
    }

    #[tokio::test]
    async fn reader_installation_does_not_require_a_document_export_folder() {
        let root = tempfile::tempdir().unwrap();
        let (actor, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
        let session = &*actor.session;
        let mut configured = options(Path::new(""), vec![AnnotationExportFormat::Foliate, AnnotationExportFormat::Readest]);
        configured.install_readers = true;
        session.set_automatic_annotation_export(Some(configured)).unwrap();
        let saved = session.automatic_annotation_export().unwrap().options.unwrap();
        assert!(saved.install_readers);
        assert!(saved.destination.as_os_str().is_empty());
    }

    #[tokio::test]
    async fn annotation_edits_load_and_export_only_the_affected_book() {
        let root = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let (actor, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
        let session = &*actor.session;
        session.db.set_asset_storage_policy(false, false).unwrap();
        session.sync.set_asset_storage_enabled(false);
        let mut notes = Vec::new();
        for title in ["First", "Second"] {
            let bytes = format!(r#"<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0"><description><title-info><book-title>{title}</book-title><lang>en</lang></title-info></description><body><section><p>A passage</p></section></body></FictionBook>"#).into_bytes();
            let book = session.import.import_downloaded_book(ROOT_DIR_ID, &format!("{title}.fb2"), bytes).await.unwrap();
            let mut annotation = note(book, AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2,/1:0,/1:5)"));
            annotation.id = title.into();
            session.db.upsert_annotation(&annotation).unwrap();
            notes.push(annotation);
        }
        session.set_automatic_annotation_export(Some(options(output.path(), vec![AnnotationExportFormat::Markdown]))).unwrap();
        let receipts_path = root.path().join("receipts.json");
        let mut receipts = Vec::new();
        assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
        let other_path = receipts.iter().find(|receipt| receipt.destination.book_id == notes[1].content_hash).unwrap().destination.path.clone();
        let other_bytes = fs::read(&other_path).unwrap();
        // A read trap in the other book proves that no full-library snapshot
        // or post-reconciliation reload is used for the next annotation edit.
        let connection = rusqlite::Connection::open(session.db.path()).unwrap();
        let changed = connection.execute("UPDATE local_book_draft SET body=x'00' WHERE content_hash=?1", [notes[1].content_hash]).unwrap()
            + connection.execute("UPDATE sync_book_record SET body=x'00' WHERE content_hash=?1", [notes[1].content_hash]).unwrap();
        assert!(changed > 0);
        assert!(session.db.annotation_export_snapshot().is_err());
        notes[0].note = "Only this book changed".into();
        notes[0].modified_at += 1;
        session.db.upsert_annotation(&notes[0]).unwrap();
        assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
        let first_path = &receipts.iter().find(|receipt| receipt.destination.book_id == notes[0].content_hash).unwrap().destination.path;
        assert!(fs::read_to_string(first_path).unwrap().contains("Only this book changed"));
        assert_eq!(fs::read(other_path).unwrap(), other_bytes);
        assert!(session.runtime.automatic_export.pending.borrow().is_empty());
    }

    #[tokio::test]
    async fn automatic_exports_import_external_notes_and_settle_without_feedback() {
        let root = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let (actor, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
        let session = &*actor.session;
        session.db.set_asset_storage_policy(false, false).unwrap();
        session.sync.set_asset_storage_enabled(false);
        let book = session.import.import_downloaded_book(ROOT_DIR_ID, "Book.epub", include_bytes!("../../tests/fixtures/cloud-cover.epub").to_vec()).await.unwrap();
        let mut annotation = note(book, AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2,/1:0,/1:5)"));
        session.db.upsert_annotation(&annotation).unwrap();
        session.set_automatic_annotation_export(Some(options(output.path(), vec![AnnotationExportFormat::Markdown, AnnotationExportFormat::Readest]))).unwrap();
        let receipts_path = root.path().join("receipts.json");
        let mut receipts = Vec::new();
        pass(session, &mut receipts, &receipts_path).await.unwrap();
        let readest = receipts.iter().find(|receipt| receipt.destination.format == AnnotationExportFormat::Readest).unwrap().destination.path.clone();
        let markdown = receipts.iter().find(|receipt| receipt.destination.format == AnnotationExportFormat::Markdown).unwrap().destination.path.clone();
        let mut data: serde_json::Value = read_json(&readest).unwrap().unwrap();
        data["annotations"][0]["note"] = "Changed in Readest".into();
        data["annotations"].as_array_mut().unwrap().push(serde_json::json!({
            "id":"external", "type":"annotation", "cfi":"epubcfi(/6/2!/4/4,/1:0,/1:5)",
            "text":"External quote", "note":"External note", "style":"highlight", "color":"yellow", "createdAt":1, "updatedAt":2,
        }));
        fs::write(&readest, serde_json::to_vec(&data).unwrap()).unwrap();
        // Bokheim retains known annotations; only the new external note is imported.
        annotation.color = 0x123456;
        annotation.modified_at += 1;
        session.db.upsert_annotation(&annotation).unwrap();
        let watermark = session.db.sync_outbox_watermark().unwrap();
        pass(session, &mut receipts, &receipts_path).await.unwrap();
        let imported = session.db.annotations(book).unwrap();
        assert_eq!(imported.len(), 2);
        let edited = imported.iter().find(|value| value.id == annotation.id).unwrap();
        assert_eq!(edited.note, annotation.note);
        assert_eq!(edited.color, annotation.color);
        let rewritten: serde_json::Value = read_json(&readest).unwrap().unwrap();
        let known = rewritten["annotations"].as_array().unwrap().iter().find(|value| value["id"].as_str() == Some(annotation.id.as_str())).unwrap();
        assert_eq!(known["note"], annotation.note);
        assert!(session.db.sync_outbox_watermark().unwrap() > watermark);
        assert!(fs::read_to_string(&markdown).unwrap().contains("External note"));
        // The imported book is queued again; its refreshed receipt prevents another write.
        pass(session, &mut receipts, &receipts_path).await.unwrap();
        fs::write(&markdown, "No repeated export after reconciliation").unwrap();
        let watermark = session.db.sync_outbox_watermark().unwrap();
        assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
        assert_eq!(fs::read_to_string(markdown).unwrap(), "No repeated export after reconciliation");
        assert_eq!(session.db.sync_outbox_watermark().unwrap(), watermark);
    }

    #[tokio::test]
    async fn automatic_exports_track_edits_deletions_restart_receipts_and_disabling() {
        let root = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let (actor, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
        let session = &*actor.session;
        assert!(session.automatic_annotation_export().unwrap().options.is_none());
        assert!(session.set_automatic_annotation_export(Some(options(&root.path().join("inside-library"), vec![AnnotationExportFormat::Markdown]))).is_err());
        assert!(!root.path().join("inside-library").exists());
        session.db.set_asset_storage_policy(false, false).unwrap();
        session.sync.set_asset_storage_enabled(false);
        let book = session.import.import_downloaded_book(ROOT_DIR_ID, "Book.epub", include_bytes!("../../tests/fixtures/cloud-cover.epub").to_vec()).await.unwrap();
        let mut annotation = note(book, AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2,/1:0,/1:5)"));
        session.db.upsert_annotation(&annotation).unwrap();
        session.set_automatic_annotation_export(Some(options(output.path(), vec![AnnotationExportFormat::Markdown, AnnotationExportFormat::Readest]))).unwrap();
        session.runtime.automatic_export.configuration.replace(None);
        assert!(session.automatic_annotation_export().unwrap().options.is_some(), "settings survive a fresh load");
        let receipts_path = root.path().join("receipts.json");
        let mut receipts = Vec::new();
        let before = outbox(session);
        assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
        assert_eq!(receipts.len(), 2);
        let markdown = receipts.iter().find(|receipt| receipt.destination.format == AnnotationExportFormat::Markdown).unwrap().destination.path.clone();
        let readest = receipts.iter().find(|receipt| receipt.destination.format == AnnotationExportFormat::Readest).unwrap().destination.path.clone();
        assert!(markdown.starts_with(output.path()));
        assert_eq!(outbox(session), before);
        receipts = read_json(&receipts_path).unwrap().unwrap();
        fs::write(&markdown, "Unchanged books must not be rewritten").unwrap();
        assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
        assert_eq!(fs::read_to_string(&markdown).unwrap(), "Unchanged books must not be rewritten");
        annotation.note = "Edited note".into();
        annotation.modified_at += 1;
        session.db.upsert_annotation(&annotation).unwrap();
        pass(session, &mut receipts, &receipts_path).await.unwrap();
        assert!(fs::read_to_string(&markdown).unwrap().contains("Edited note"));
        let data: serde_json::Value = read_json(&readest).unwrap().unwrap();
        assert_eq!(data["annotations"][0]["note"], "Edited note");
        annotation.note = "Export survives a receipt storage failure".into();
        annotation.modified_at += 1;
        session.db.upsert_annotation(&annotation).unwrap();
        assert!(pass(session, &mut receipts, &root.path().join("missing/receipts.json")).await.is_err());
        assert!(fs::read_to_string(&markdown).unwrap().contains("Export survives a receipt storage failure"));
        fs::write(&markdown, "Do not repeat completed exports when saving receipts retries").unwrap();
        assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
        assert_eq!(fs::read_to_string(&markdown).unwrap(), "Do not repeat completed exports when saving receipts retries");
        session.db.delete_annotation(&annotation.id, 5).unwrap();
        pass(session, &mut receipts, &receipts_path).await.unwrap();
        let data: serde_json::Value = read_json(&readest).unwrap().unwrap();
        assert_eq!(data["annotations"].as_array().unwrap().len(), 0);
        assert!(!fs::read_to_string(&markdown).unwrap().contains("Export survives a receipt storage failure"));
        session.set_automatic_annotation_export(None).unwrap();
        annotation.modified_at = 6;
        session.db.upsert_annotation(&annotation).unwrap();
        pass(session, &mut receipts, &receipts_path).await.unwrap();
        assert!(!fs::read_to_string(&markdown).unwrap().contains("Export survives a receipt storage failure"));
    }

    #[tokio::test]
    async fn automatic_export_failures_survive_restart_pause_unchanged_inputs_and_retry_independently() {
        use library_model::AutomaticAnnotationExportState;
        let root = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let (actor, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
        let session = &*actor.session;
        session.db.set_asset_storage_policy(false, false).unwrap();
        session.sync.set_asset_storage_enabled(false);
        let book = session.import.import_downloaded_book(ROOT_DIR_ID, "Book.epub", include_bytes!("../../tests/fixtures/cloud-cover.epub").to_vec()).await.unwrap();
        let mut annotation = note(book, AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2,/1:0,/1:5)"));
        session.db.upsert_annotation(&annotation).unwrap();
        let options = options(output.path(), vec![AnnotationExportFormat::Markdown, AnnotationExportFormat::Readest, AnnotationExportFormat::Foliate]);
        let destinations = plan(session.id, &session.db.annotation_export_snapshot().unwrap(), &options).unwrap().items;
        let path = |format| destinations.iter().find(|item| item.format == format).unwrap().path.clone();
        let markdown = path(AnnotationExportFormat::Markdown);
        let readest = path(AnnotationExportFormat::Readest);
        let foliate = path(AnnotationExportFormat::Foliate);
        session.set_automatic_annotation_export(Some(options.clone())).unwrap();
        let receipts_path = root.path().join("receipts.json");
        let mut receipts = Vec::new();
        assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
        let readest_original = fs::read(&readest).unwrap();
        let foliate_original = fs::read(&foliate).unwrap();
        fs::write(&readest, b"invalid JSON").unwrap();
        let mut wrong: serde_json::Value = serde_json::from_slice(&foliate_original).unwrap();
        wrong["_bokheim_book_id"] = serde_json::to_value(BookId::from(blake3::hash(b"another book"))).unwrap();
        fs::write(&foliate, serde_json::to_vec(&wrong).unwrap()).unwrap();
        annotation.note = "Latest Bokheim note".into();
        annotation.modified_at += 1;
        session.db.upsert_annotation(&annotation).unwrap();
        assert!(pass(session, &mut receipts, &receipts_path).await.unwrap());
        let status = session.automatic_annotation_export().unwrap();
        assert_eq!(status.outputs.len(), 2);
        assert!(status.outputs.iter().all(|item| item.state == AutomaticAnnotationExportState::NeedsAttention && item.attempts == 1));
        assert!(status.outputs.iter().any(|item| item.failure.kind == AnnotationExportFailureKind::Identity));
        assert!(status.outputs.iter().any(|item| item.failure.kind == AnnotationExportFailureKind::InvalidData));
        assert!(status.last_success_at.is_some());
        assert!(fs::read_to_string(&markdown).unwrap().contains("Latest Bokheim note"));
        session.runtime.automatic_export.status.replace(None);
        assert_eq!(session.automatic_annotation_export().unwrap().outputs, status.outputs, "per-output failures survive a fresh load");
        // Saving identical settings after a partial manual export must not hide failures.
        session.set_automatic_annotation_export(Some(options)).unwrap();
        assert_eq!(session.automatic_annotation_export().unwrap().outputs, status.outputs);
        fs::write(&markdown, "Keep successful outputs untouched").unwrap();
        assert!(pass(session, &mut receipts, &receipts_path).await.unwrap());
        assert_eq!(session.automatic_annotation_export().unwrap().outputs, status.outputs, "unchanged invalid files are not reparsed");
        session.retry_automatic_annotation_export().unwrap();
        assert!(pass(session, &mut receipts, &receipts_path).await.unwrap());
        assert!(session.automatic_annotation_export().unwrap().outputs.iter().all(|item| item.attempts == 2));
        fs::write(&readest, readest_original).unwrap();
        assert!(pass(session, &mut receipts, &receipts_path).await.unwrap());
        let status = session.automatic_annotation_export().unwrap();
        assert_eq!(status.outputs.len(), 1);
        assert_eq!(status.outputs[0].destination.format, AnnotationExportFormat::Foliate);
        assert_eq!(status.outputs[0].attempts, 2, "repairing one file must not retry another paused file");
        fs::write(&foliate, foliate_original).unwrap();
        assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
        assert!(session.automatic_annotation_export().unwrap().outputs.is_empty());
        assert_eq!(fs::read_to_string(&markdown).unwrap(), "Keep successful outputs untouched");
    }

    #[tokio::test]
    async fn automatic_export_io_failures_back_off_and_explicit_retry_resumes_after_repair() {
        use library_model::AutomaticAnnotationExportState;
        let root = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let (actor, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
        let session = &*actor.session;
        session.db.set_asset_storage_policy(false, false).unwrap();
        session.sync.set_asset_storage_enabled(false);
        let book = session.import.import_downloaded_book(ROOT_DIR_ID, "Book.epub", include_bytes!("../../tests/fixtures/cloud-cover.epub").to_vec()).await.unwrap();
        session.db.upsert_annotation(&note(book, AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2,/1:0,/1:5)"))).unwrap();
        let blocked_folder = output.path().join("Blocked");
        let options = options(&blocked_folder, vec![AnnotationExportFormat::Markdown]);
        session.set_automatic_annotation_export(Some(options)).unwrap();
        fs::write(&blocked_folder, "a file occupies the export folder").unwrap();
        let receipts_path = root.path().join("receipts.json");
        let mut receipts = Vec::new();
        assert!(pass(session, &mut receipts, &receipts_path).await.unwrap());
        let status = session.automatic_annotation_export().unwrap();
        assert_eq!(status.outputs.len(), 1);
        let first = &status.outputs[0];
        assert_eq!(first.state, AutomaticAnnotationExportState::Retrying);
        assert_eq!(first.failure.kind, AnnotationExportFailureKind::Io);
        assert_eq!(first.next_retry_at.unwrap() - first.last_attempt_at, 30_000);
        assert!(pass(session, &mut receipts, &receipts_path).await.unwrap());
        assert_eq!(session.automatic_annotation_export().unwrap().outputs[0].attempts, 1);
        let mut state = status::load(session).unwrap();
        state.failures[0].output.next_retry_at = Some(0);
        status::publish(session, state).unwrap();
        assert!(pass(session, &mut receipts, &receipts_path).await.unwrap());
        let status = session.automatic_annotation_export().unwrap();
        assert_eq!(status.outputs[0].attempts, 2);
        assert_eq!(status.outputs[0].next_retry_at.unwrap() - status.outputs[0].last_attempt_at, 60_000);
        fs::remove_file(&blocked_folder).unwrap();
        fs::create_dir(&blocked_folder).unwrap();
        session.retry_automatic_annotation_export().unwrap();
        assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
        assert!(session.automatic_annotation_export().unwrap().outputs.is_empty());
        assert_eq!(receipts.len(), 1);
    }

    #[tokio::test]
    async fn automatic_pdf_exports_while_reading_and_retries_busy_publications() {
        let root = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let (actor, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
        let session = &*actor.session;
        let mut document = Document::with_version("1.7");
        let pages = document.new_object_id();
        let page = document.add_object(dictionary! {"Type"=>"Page","Parent"=>pages});
        document.objects.insert(pages, dictionary! {"Type"=>"Pages","Kids"=>vec![Object::Reference(page)],"Count"=>1,"MediaBox"=>vec![0.into(),0.into(),600.into(),800.into()]}.into());
        let catalog = document.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
        document.trailer.set("Root", catalog);
        let path = root.path().join("Book.pdf");
        document.save(&path).unwrap();
        let original = fs::read(&path).unwrap();
        let checksum = blake3::hash(&original).into();
        let book = BookId::for_local_content(session.id.as_bytes(), checksum);
        session
            .db
            .commit_import(ImportCommit {
                source_identity: None,
                byte_checksum: Some(checksum),
                parent_id: ROOT_DIR_ID,
                file_name: "Book.pdf".into(),
                format: BookFormat::Pdf,
                content_hash: book,
                size_bytes: original.len() as u64,
                inspection_version: 1,
                inspection: Some(book_inspection::InspectedBook {
                    metadata: book_model::BookRecord { metadata: BookDetails { title: "PDF book".into(), ..Default::default() }, description: String::new() },
                    pdf: Some(PdfReaderMetadata { version: 1, content_hash: book.to_hex(), pages: vec![PdfStoredPage { width: 600.0, height: 800.0, rotation: 0 }], skippable_pages: None, annotations: vec![], dependencies: None }),
                    audiobook: None,
                    toc: vec![],
                }),
                published: PublishedStagedBook { name: "Book.pdf".into(), relative_path: "/Book.pdf".into(), published: true, fingerprint: None },
                request_thumbnail: false,
                restore_paths: vec![],
            })
            .unwrap();
        // Opening must wait for an in-flight publisher, then release its
        // short lease before returning the seekable reader to the renderer.
        let publishing = session.assets.try_exclusive_lease(&book).unwrap().unwrap();
        let opening = session.prepare_book_for_reader(book);
        futures_util::pin_mut!(opening);
        assert!(timeout(Duration::from_millis(100), &mut opening).await.is_err(), "reader must wait for publication before selecting a revision");
        drop(publishing);
        let opened = opening.await.unwrap().into_document().unwrap();
        assert!(opened.pdf_metadata.is_some(), "the initial revision has matching cached metadata");
        let mut reader = opened.source.into_reader().unwrap();
        assert!(session.assets.try_exclusive_lease(&book).unwrap().is_some(), "an open reader must not block PDF export");

        let annotation = note(book, AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(0, vec![PdfAnnotationRect::new(0.1, 0.2, 0.3, 0.1)], None)));
        session.db.upsert_annotation(&annotation).unwrap();
        session.set_automatic_annotation_export(Some(options(output.path(), vec![AnnotationExportFormat::Markdown, AnnotationExportFormat::Pdf]))).unwrap();
        let receipts_path = root.path().join("receipts.json");
        let mut receipts = Vec::new();
        let publication = session.assets.lease_book(&book).await.unwrap();
        let before = outbox(session);
        assert!(pass(session, &mut receipts, &receipts_path).await.unwrap(), "busy PDFs remain pending");
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(receipts.len(), 1, "Markdown can finish while PDF publication is busy");
        let markdown = output.path().join("PDF book.md");
        fs::write(&markdown, "No repeated writes while publication remains busy").unwrap();
        assert!(pass(session, &mut receipts, &receipts_path).await.unwrap());
        assert_eq!(fs::read_to_string(&markdown).unwrap(), "No repeated writes while publication remains busy");
        drop(publication);
        assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
        assert_eq!(receipts.len(), 2);
        let changed = session.db.local_content_hash(book, ROOT_DIR_ID).unwrap().unwrap();
        assert_ne!(changed, checksum);
        assert!(fs::read_to_string(&markdown).unwrap().contains(&format!("local_content_hash: \"{changed}\"")));
        let pdf = fs::read(&path).unwrap();
        let mut pinned = Vec::new();
        reader.read_to_end(&mut pinned).unwrap();
        assert_eq!(pinned, original, "the active reader must retain its original PDF after background export");
        fs::write(&markdown, "No repeated writes").unwrap();
        assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
        assert_eq!(fs::read(&path).unwrap(), pdf);
        assert_eq!(fs::read_to_string(&markdown).unwrap(), "No repeated writes");
        assert_eq!(outbox(session), before);
        assert_eq!(session.db.annotations(book).unwrap(), vec![annotation.clone()]);

        let mut edited = annotation;
        edited.note = "Manually exported edit".into();
        edited.modified_at += 1;
        session.db.upsert_annotation(&edited).unwrap();
        let job = session.prepare_annotation_export(uuid::Uuid::new_v4()).unwrap();
        let report = session.export_annotations(options(output.path(), vec![AnnotationExportFormat::Pdf, AnnotationExportFormat::Markdown]), job).await.unwrap();
        assert!(report.results.iter().all(|result| matches!(result.outcome, AnnotationExportOutcome::Written)));
        fs::write(&markdown, "The initial manual export must not be repeated").unwrap();
        assert!(!pass(session, &mut receipts, &receipts_path).await.unwrap());
        assert_eq!(fs::read_to_string(&markdown).unwrap(), "The initial manual export must not be repeated");
        reader.rewind().unwrap();
        pinned.clear();
        reader.read_to_end(&mut pinned).unwrap();
        assert_eq!(pinned, original, "further exports must not invalidate the active reader's snapshot");
        let reopened = session.prepare_book_for_reader(book).await.unwrap().into_document().unwrap();
        assert!(reopened.pdf_metadata.is_none(), "a rewritten PDF must not reuse its original cached metadata");
        let mut latest = Vec::new();
        reopened.source.into_reader().unwrap().read_to_end(&mut latest).unwrap();
        assert_eq!(latest, fs::read(&path).unwrap(), "new readers must see the exported revision");
        assert_ne!(latest, original);
    }

    #[tokio::test]
    async fn worker_coalesces_local_commands_and_received_annotation_changes() {
        use crate::library::{
            commands::{prepare_library_request, PreparedRequest},
            events::LibraryEvent,
            LibraryCommand, ReadyCommand,
        };
        let root = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let (mut actor, _, _) = fixture(root.path(), "http://127.0.0.1:1".parse().unwrap());
        let session = actor.session.clone();
        session.db.set_asset_storage_policy(false, false).unwrap();
        session.sync.set_asset_storage_enabled(false);
        let book = session.import.import_downloaded_book(ROOT_DIR_ID, "Book.epub", include_bytes!("../../tests/fixtures/cloud-cover.epub").to_vec()).await.unwrap();
        let mut annotation = note(book, AnnotationAnchor::epub_publication_cfi("epubcfi(/6/2!/4/2,/1:0,/1:5)"));
        session.db.upsert_annotation(&annotation).unwrap();
        let options = options(output.path(), vec![AnnotationExportFormat::Markdown]);
        let path = plan(session.id, &session.db.annotation_export_snapshot().unwrap(), &options).unwrap().items.remove(0).path;
        session.set_automatic_annotation_export(Some(options)).unwrap();
        let exercise = async {
            timeout(Duration::from_secs(5), async {
                while !fs::read_to_string(&path).unwrap_or_default().contains("First note") {
                    sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
            annotation.note = "Intermediate edit".into();
            annotation.modified_at += 1;
            let result = prepare_library_request(&mut actor, LibraryCommand::Ready(ReadyCommand::UpsertAnnotation { annotation: annotation.clone() })).unwrap();
            let PreparedRequest::Ready(reply) = result else { panic!("annotation edits must commit immediately") };
            reply.take::<()>().unwrap();
            sleep(Duration::from_millis(100)).await;
            annotation.note = "Received edit".into();
            annotation.modified_at += 1;
            session.db.upsert_annotation(&annotation).unwrap();
            session.events.emit(LibraryEvent::ContentsChanged);
            actor.coordination.drain(&session.events);
            sleep(Duration::from_millis(200)).await;
            assert!(fs::read_to_string(&path).unwrap().contains("First note"), "nearby edits must coalesce");
            timeout(Duration::from_secs(5), async {
                while !fs::read_to_string(&path).unwrap_or_default().contains("Received edit") || session.runtime.automatic_export.active.borrow().is_some() {
                    sleep(Duration::from_millis(20)).await;
                }
            })
            .await
            .unwrap();
        };
        tokio::select! {
            _ = run(&session) => panic!("the worker must remain alive"),
            _ = exercise => {},
        }
    }
}
