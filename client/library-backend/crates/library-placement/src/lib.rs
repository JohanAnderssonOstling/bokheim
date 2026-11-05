//! Where a library's books and folders live: folder organization, and the
//! database-backed placement policy that decides which storage operations
//! (file moves, folder replay, eviction, staging and publishing imports) are
//! required. `library-files` performs the storage operations themselves.

#[cfg(not(target_arch = "wasm32"))]
use std::{error::Error, io::ErrorKind};

use library_database::DatabaseClient;
use library_files::AssetStore;
use library_services::events::LibraryEventRouter;

mod asset_workflow;
#[cfg(not(target_arch = "wasm32"))]
mod import_directories;
mod management;
#[cfg(not(target_arch = "wasm32"))]
mod native_edit;
#[cfg(not(target_arch = "wasm32"))]
pub use import_directories::StagedImportDirectories;

pub use asset_workflow::{evict_local_book, publish_import, run_file_jobs, run_file_jobs_for, stage_import};
#[cfg(not(target_arch = "wasm32"))]
pub use asset_workflow::{prepare_directory, replay_directories};

/// Shared database, storage, and event handles for one library.
/// Cloning snapshots the current storage handles so placement I/O can run
/// without retaining the library state lock.
#[derive(Clone)]
pub struct LibraryState {
    database: DatabaseClient,
    assets: AssetStore,
    events: LibraryEventRouter,
}

impl LibraryState {
    pub fn new(database: DatabaseClient, assets: AssetStore, events: LibraryEventRouter) -> Self {
        Self { database, assets, events }
    }
    pub fn database(&self) -> &DatabaseClient {
        &self.database
    }
    pub fn assets(&self) -> &AssetStore {
        &self.assets
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn replace_assets(&mut self, assets: AssetStore) {
        self.assets.retire();
        self.assets = assets;
    }
    pub fn events(&self) -> &LibraryEventRouter {
        &self.events
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn crosses_devices(mut error: &(dyn Error + 'static)) -> bool {
    loop {
        if error.downcast_ref::<std::io::Error>().is_some_and(|error| error.kind() == ErrorKind::CrossesDevices) {
            return true;
        }
        let Some(source) = error.source() else { return false };
        error = source;
    }
}
