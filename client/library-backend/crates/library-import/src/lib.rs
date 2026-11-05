//! Bringing books into a library: dropped files and folders placed on disk
//! and imported in place, files a caller already stored, and browser
//! selected folders. Every route validates the destination, inspects the
//! book, publishes its bytes, and commits the addition the same way.
//!
//! The operation scheduler (see `library-backend`) batches contents
//! notifications until the admitted import completes.

use std::sync::Arc;

use library_placement::LibraryState;
use library_services::events::LibraryEventRouter;

mod catalog;
mod completion;
#[cfg(not(target_arch = "wasm32"))]
mod filesystem_import;
mod import;
#[cfg(not(target_arch = "wasm32"))]
mod import_native;
mod import_status;
#[cfg(not(target_arch = "wasm32"))]
mod native_move;
#[cfg(not(target_arch = "wasm32"))]
pub use native_move::NativeMoveResult;
#[cfg(not(target_arch = "wasm32"))]
pub mod foliate;
#[cfg(not(target_arch = "wasm32"))]
mod reader_batch;
#[cfg(not(target_arch = "wasm32"))]
pub mod reader_import;
#[cfg(not(target_arch = "wasm32"))]
mod reader_transfer;
#[cfg(not(target_arch = "wasm32"))]
pub mod readest;
#[cfg(target_arch = "wasm32")]
mod selected;
mod transfer;

pub use import_status::{ImportFailure, ImportFailureKind};

#[cfg(not(target_arch = "wasm32"))]
pub use client_platform_native::filesystem::{ImportReader as DirectoryImportReader, ImportReaderFuture as DirectoryImportReaderFuture, ImportSource};
#[cfg(target_arch = "wasm32")]
pub use client_platform_web::transport::import_io::{ImportReader as DirectoryImportReader, ImportReaderFuture as DirectoryImportReaderFuture, ImportSource};

pub struct LibraryImport<'a> {
    storage: &'a LibraryState,
    cpu: Arc<dyn cpu_host::CpuHost>,
    events: LibraryEventRouter,
    /// The library folder dropped files are placed into.
    #[cfg(not(target_arch = "wasm32"))]
    root: std::path::PathBuf,
}

impl<'a> LibraryImport<'a> {
    /// Borrow the caller's storage handles for the duration of the import.
    pub fn new(storage: &'a LibraryState, cpu: Arc<dyn cpu_host::CpuHost>) -> Self {
        Self {
            storage,
            events: storage.events().clone(),
            #[cfg(not(target_arch = "wasm32"))]
            root: storage.assets().native_library_root().to_path_buf(),
            cpu,
        }
    }
}

/// A selected directory tree whose files can be opened one at a time while
/// the backend imports them.
pub struct DirectoryImport {
    pub name: String,
    pub directories: Vec<Vec<String>>,
    pub files: Vec<DirectoryImportFile>,
}

/// A source capability and its path within the selected tree.
pub struct DirectoryImportFile {
    pub path: Vec<String>,
    pub source: ImportSource,
}

/// The folders created for a selected tree, by path, before any file is opened.
#[cfg(target_arch = "wasm32")]
pub(crate) struct PreparedDirectoryImport {
    pub(crate) directories: std::collections::HashMap<Vec<String>, sync_common::DirId>,
    pub(crate) failures: Vec<ImportFailure>,
}
