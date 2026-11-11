//! Values shared by the export workflow and its future user interface.
use book_model::{Annotation, BookDetails, BookFormat, BookTocEntry};
use content_address::{BookId, ContentHash};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::{Uuid as DirId, Uuid as LibraryId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnnotationExportFormat {
    Markdown,
    Pdf,
    Foliate,
    Readest,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnnotationExportConflict {
    #[default]
    KeepBoth,
    Replace,
    Skip,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnotationExportOptions {
    /// Final library export folder, independently editable from the default base.
    pub destination: PathBuf,
    pub formats: Vec<AnnotationExportFormat>,
    /// Reader formats update installed applications. False retains portable
    /// JSON output for transfers and explicit backend callers.
    #[serde(default = "install_readers_by_default")]
    pub install_readers: bool,
    pub conflict: AnnotationExportConflict,
    #[serde(default)]
    pub overrides: Vec<AnnotationExportDestination>,
}

fn install_readers_by_default() -> bool {
    true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnnotationExportFailureKind {
    Busy,
    Unavailable,
    Changed,
    InvalidData,
    Identity,
    Unsupported,
    LimitExceeded,
    Io,
    Tracking,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnotationExportFailure {
    pub kind: AnnotationExportFailureKind,
    /// Publication succeeded, but recording or flushing its result failed.
    pub file_written: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<operation_failure::Failure>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnnotationExportDestination {
    pub book_id: BookId,
    pub directory_id: DirId,
    pub format: AnnotationExportFormat,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnnotationExportPlacement {
    pub directory_id: DirId,
    pub relative_path: String,
    pub downloaded: bool,
    pub local_hash: Option<ContentHash>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnnotationExportBook {
    pub book_id: BookId,
    pub format: BookFormat,
    pub metadata: BookDetails,
    pub toc: Vec<BookTocEntry>,
    pub annotations: Vec<Annotation>,
    pub canonical_hash: Option<ContentHash>,
    pub placements: Vec<AnnotationExportPlacement>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnnotationExportPlan {
    pub library_id: LibraryId,
    pub items: Vec<AnnotationExportDestination>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AnnotationExportOutcome {
    Written,
    Skipped,
    Failed(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnnotationExportResult {
    pub destination: AnnotationExportDestination,
    pub outcome: AnnotationExportOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<AnnotationExportFailure>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AnnotationExportReport {
    pub results: Vec<AnnotationExportResult>,
}

impl std::fmt::Display for AnnotationExportFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(formatter, "{:?}", self.kind) }
}
impl operation_failure::Classified for AnnotationExportFailure {
    fn failure(&self) -> Option<operation_failure::Failure> { self.cause }
}
