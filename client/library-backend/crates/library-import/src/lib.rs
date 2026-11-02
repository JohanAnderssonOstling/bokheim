//! Bringing books into a library: dropped files and folders placed on disk
//! and imported in place, files a caller already stored, and browser
//! selected folders. Every route validates the destination, inspects the
//! book, publishes its bytes, and commits the addition the same way.
//!
//! The session brackets each import (see `library-backend`) so contents
//! notifications wait for its one completion announcement.

use std::sync::Arc;

use library_database::Database;
use library_files::AssetStore;
use library_placement::LibraryPlacement;
use library_runtime::events::LibraryEventRouter;

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

pub struct LibraryImport {
    database: Arc<Database>,
    assets: AssetStore,
    cpu: Arc<dyn cpu_host::CpuHost>,
    events: LibraryEventRouter,
    placement: Arc<LibraryPlacement>,
    /// The library folder dropped files are placed into.
    #[cfg(not(target_arch = "wasm32"))]
    root: std::path::PathBuf,
}

impl LibraryImport {
    /// Import and placement share one storage owner. In particular, callers
    /// cannot pair its placement policy with another database, root or router.
    pub fn new(placement: Arc<LibraryPlacement>, cpu: Arc<dyn cpu_host::CpuHost>) -> Self {
        Self {
            database: placement.database().clone(),
            assets: placement.assets().clone(),
            events: placement.events().clone(),
            #[cfg(not(target_arch = "wasm32"))]
            root: placement.assets().native_library_root().to_path_buf(),
            placement,
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
