pub use publication_access::{BookReader, BoxedBookReader};

/// A selected directory tree whose files can be opened one at a time while
/// the backend imports them.
pub struct DirectoryImport {
    pub name: String,
    pub directories: Vec<Vec<String>>,
    pub files: Vec<DirectoryImportFile>,
}

pub use crate::platform::{DirectoryImportReader, DirectoryImportReaderFuture, ImportSource};

/// A source capability and its path within the selected tree.
pub struct DirectoryImportFile {
    pub path: Vec<String>,
    pub source: ImportSource,
}

/// The small directory manifest returned before any selected file is opened.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub(crate) struct PreparedDirectoryImport {
    pub directories: Vec<(Vec<String>, crate::DirId)>,
    pub failures: Vec<crate::ImportFailure>,
}
