use std::future::Future;
use std::io::{Read, Seek};
use std::pin::Pin;

/// Seekable publication content opened by a library backend.
pub trait BookReader: Read + Seek + Send + Sync {}
impl<T: Read + Seek + Send + Sync> BookReader for T {}

pub type BoxedBookReader = Box<dyn BookReader>;

/// A selected directory tree whose files can be opened one at a time while
/// the backend imports them.
pub struct DirectoryImport {
    pub name: String,
    pub directories: Vec<Vec<String>>,
    pub files: Vec<DirectoryImportFile>,
}

pub type DirectoryImportReaderFuture = Pin<Box<dyn Future<Output = Result<Box<dyn Read + Send>, String>> + 'static>>;

pub struct DirectoryImportFile {
    pub path: Vec<String>,
    pub open: Box<dyn FnOnce() -> DirectoryImportReaderFuture + Send>,
}
