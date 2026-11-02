//! Shared contracts and entry points for importing libraries from other readers.
use crate::foliate::FoliateLibrary;
use crate::import::validate_import_destination;
use crate::reader_batch::ReaderImportBatch;
use crate::reader_transfer::ReaderBookResult;
use crate::readest::ReadestLibrary;
use crate::{ImportFailure, LibraryImport};
use client_platform_runtime::executor::run_blocking;
use client_runtime::BackendError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use sync_common::DirId;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum ImportMode {
    #[default]
    Copy,
    Move,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum ReaderImportSource {
    Readest,
    Foliate,
}
impl ReaderImportSource {
    pub fn name(self) -> &'static str {
        match self {
            Self::Readest => "Readest",
            Self::Foliate => "Foliate",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ReaderBookCounts {
    pub epub: usize,
    pub pdf: usize,
}

impl ReaderBookCounts {
    pub fn total(self) -> usize {
        self.epub + self.pdf
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ReaderGroup {
    pub name: String,
    /// Includes all descendant groups, with slash boundaries respected.
    pub counts: ReaderBookCounts,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReaderLibrarySummary {
    pub counts: ReaderBookCounts,
    pub groups: Vec<ReaderGroup>,
    pub can_move: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReaderImportOptions {
    pub parent_id: DirId,
    /// None means all groups; an empty string means ungrouped books.
    pub group: Option<String>,
    pub mode: ImportMode,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ReaderImportProgress {
    pub completed: usize,
    pub total: usize,
    pub current: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ReaderImportReport {
    pub imported: usize,
    pub already_present: usize,
    pub failures: Vec<ImportFailure>,
    pub warnings: Vec<String>,
    pub skipped_pdf_annotations: usize,
    pub completed: usize,
    pub total: usize,
    pub cancelled: bool,
}

pub enum ReaderLibrary {
    Readest(ReadestLibrary),
    Foliate(FoliateLibrary),
}
impl ReaderLibrary {
    pub async fn load_default(source: ReaderImportSource) -> Result<Self, BackendError> {
        run_blocking(move || match source {
            ReaderImportSource::Readest => ReadestLibrary::load_default_sync().map(Self::Readest),
            ReaderImportSource::Foliate => FoliateLibrary::load_default().map(Self::Foliate),
        })
        .await?
    }
    pub fn summary(&self) -> ReaderLibrarySummary {
        match self {
            Self::Readest(library) => library.summary(),
            Self::Foliate(library) => library.summary(),
        }
    }
    pub async fn import(self, importer: &LibraryImport, options: ReaderImportOptions, cancelled: &dyn Fn() -> bool, progress: impl FnMut(ReaderImportProgress)) -> Result<ReaderImportReport, BackendError> {
        match self {
            Self::Readest(library) => library.import(importer, options, cancelled, progress).await,
            Self::Foliate(library) => library.import(importer, options, cancelled, progress).await,
        }
    }
}

/// Reader-specific discovery and state conversion feed the same import loop.
pub(crate) trait ReaderImportBook {
    const SOURCE: ReaderImportSource;

    fn title(&self) -> &str;
    fn folders(&self) -> Result<Vec<String>, BackendError>;
    fn validate_group(_group: Option<&str>) -> Result<(), BackendError> {
        Ok(())
    }

    async fn import(
        self, importer: &LibraryImport, resolve_parent: impl FnOnce() -> Result<DirId, BackendError>, mode: ImportMode, batch: Option<&mut ReaderImportBatch>, cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<ReaderBookResult>, BackendError>;
}

pub(crate) async fn import_books<B: ReaderImportBook>(
    importer: &LibraryImport, books: Vec<B>, options: ReaderImportOptions, cancelled: &dyn Fn() -> bool, mut progress: impl FnMut(ReaderImportProgress),
) -> Result<ReaderImportReport, BackendError> {
    let mut report = ReaderImportReport { total: books.len(), ..Default::default() };
    if cancelled() {
        report.cancelled = true;
        return Ok(report);
    }
    B::validate_group(options.group.as_deref())?;
    let source = B::SOURCE.name();
    if books.is_empty() {
        return Err(BackendError::message(format!("no EPUB or PDF books were found in the selected {source} library")));
    }
    validate_import_destination(&importer.database, &options.parent_id)?;

    let mut batch = ReaderImportBatch::new(importer, options.mode)?;
    let destinations = {
        // Invalid reader paths are reported per book, before its source moves.
        let mut seen = HashSet::new();
        let paths = books.iter().filter_map(|book| book.folders().ok()).filter(|path| seen.insert(path.clone())).collect::<Vec<_>>();
        match batch.as_mut() {
            Some(batch) => batch.prepare_tree(importer, options.parent_id, &paths).await?,
            None => importer.placement.prepare_import_directories(options.parent_id, &paths).await?,
        }
    };
    for book in books {
        if cancelled() {
            report.cancelled = true;
            break;
        }
        let title = book.title().to_owned();
        progress(ReaderImportProgress { completed: report.completed, total: report.total, current: Some(title.clone()) });
        let result = async {
            let folders = book.folders()?;
            // All destinations were prepared before any source can be moved.
            let resolve_parent = || {
                if folders.is_empty() {
                    return Ok(options.parent_id);
                }
                destinations.get(&folders).copied().ok_or_else(|| BackendError::message("reader import folder was not prepared"))
            };
            book.import(importer, resolve_parent, options.mode, batch.as_mut(), cancelled).await
        }
        .await;
        match result {
            Ok(Some(imported)) => {
                if imported.created {
                    report.imported += 1;
                } else {
                    report.already_present += 1;
                }
                report.skipped_pdf_annotations += imported.skipped_pdf_annotations;
                report.warnings.extend(imported.warnings.into_iter().map(|warning| format!("{title}: {warning}")));
                if let Some(error) = imported.cleanup_error {
                    report.warnings.push(format!("{title}: {error}"));
                }
            }
            Ok(None) => {
                report.cancelled = true;
                break;
            }
            Err(error) => report.failures.push(ImportFailure::file(&[title], &error)),
        }
        report.completed += 1;
        progress(ReaderImportProgress { completed: report.completed, total: report.total, current: None });
    }
    if let Some(batch) = batch {
        batch.finish(importer, &mut report).await?;
    }
    Ok(report)
}
