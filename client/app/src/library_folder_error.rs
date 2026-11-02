//! Folder names and failed steps stay structured across the backend worker.
use crate::BackendError;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum LibraryFolderStep {
    CheckFolder,
    NotAFolder,
    ReadLibraryInfo,
    SaveLibraryInfo,
    OpenLibrary,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LibraryFolderError {
    pub folder_name: Option<String>,
    pub step: LibraryFolderStep,
    pub error: BackendError,
}

#[cfg(test)]
#[path = "../tests/library_folder_error/tests.rs"]
mod tests;
