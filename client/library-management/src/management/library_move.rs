//! Library relocation workflow and durable retirement of the previous root.

use super::LibraryService;
use crate::executor;
use crate::BackendError;
use library_database::Database;
use library_files::asset_store::BookLease;
use library_files::library_move::{book_checksum, checked_path, cleanup_root_matches, stored_relative_path, MovePlan};
use library_files::AssetStore;
use library_registry::LibraryEntry;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use sync_common::{BookId, LibraryId};

fn relative_path(path: &str) -> &Path {
    Path::new(path.trim_start_matches('/'))
}

/// Keep one exclusive guard per identity, even for books with several placements.
fn lease_books(assets: &AssetStore, books: impl IntoIterator<Item = BookId>, leases: &mut HashMap<BookId, BookLease>) -> Result<bool, BackendError> {
    for book in books {
        if let Entry::Vacant(entry) = leases.entry(book) {
            let Some(lease) = assets.try_exclusive_lease(&book)? else { return Ok(false) };
            entry.insert(lease);
        }
    }
    Ok(true)
}

impl LibraryService {
    pub async fn move_library(self: &Arc<Self>, id: LibraryId, destination: String) -> Result<LibraryEntry, BackendError> {
        let _location = self.config.library_location_lock.lock().await;
        let _copy = self.config.storage_copy_lock.lock().await;
        let entry = self.entry(&id).await?;
        // Validate before pausing file work or writing anything to either root.
        let source = PathBuf::from(entry.storage_locator());
        let source_path = source.clone();
        let destination = PathBuf::from(destination);
        let destination = self.config.registry.access(move |registry| registry.validate_move_destination(id, source_path, destination)).await.map_err(BackendError::operation)?;
        let source_root = source.clone();
        let handle = self.handle(&id)?;
        let backend = self.clone();
        let original_root = entry.storage_locator().to_owned();
        let result = handle
            .move_root(
                Box::new(move |source_identity| match backend.move_library_files(id, source, destination, source_identity) {
                    Ok(root) => (root, Ok(())),
                    Err(error) => (original_root, Err(error)),
                }),
            )
            .await;
        self.config.library_list_changed.send_replace(());
        if let Err(error) = result {
            let context = if self.entry(&id).await.is_ok_and(|entry| Path::new(entry.storage_locator()) == source_root) {
                "Library move preparation failed. The original library was retained; retry this destination or select a different folder."
            } else {
                "The library moved, but reopening its new folder failed. Reconnect the destination folder and retry opening the library."
            };
            return Err(error.context(context));
        }
        self.entry(&id).await
    }

    fn move_library_files(&self, id: LibraryId, source: PathBuf, destination: PathBuf, active_identity: String) -> Result<String, BackendError> {
        self.config.registry.validate_move_destination(id, source.clone(), destination.clone()).map_err(BackendError::operation)?;
        let database = self.config.registry.library_database_locator(&id, source.to_string_lossy().as_ref());
        let db = Database::open(&database)?;
        let directory_ids = db.registered_library_directory_ids()?;
        // Check before opening the old store, which can create metadata.
        if !cleanup_root_matches(&source, &active_identity, id)? {
            return Err("source storage changed before the move; original files were retained".into());
        }
        let assets = AssetStore::open(source.to_string_lossy().as_ref())?;
        if assets.native_storage_identity() != active_identity {
            return Err("source storage changed before the move; original files were retained".into());
        }
        let mut inventory = Vec::new();
        let mut leases = HashMap::new();
        for (book, relative) in db.registered_library_files()? {
            // Remote-only placements have no source bytes to protect.
            let path = checked_path(&source, relative_path(relative.as_str()))?;
            if !path.try_exists()? {
                continue;
            }
            if !lease_books(&assets, [book], &mut leases)? {
                return Err("a book is in use; close it and retry moving the library".into());
            }
            inventory.push((book, relative));
        }
        let mut plan = MovePlan::prepare(source.clone(), destination.clone())?;
        let relocated_directories = plan.relocated_directories(&directory_ids)?;
        // Serialize against an independently running instance using this destination.
        let destination_assets = AssetStore::open(destination.to_string_lossy().as_ref())?;
        let mut destination_leases = HashMap::new();
        if !lease_books(&destination_assets, leases.keys().copied(), &mut destination_leases)? {
            return Err("a destination book is in use; retry the library move".into());
        }
        let transfer = plan.transfer(id, destination_assets.native_storage_identity().to_owned())?;
        plan.validate_commit()?;
        let mut relocated_files = Vec::new();
        for (book, relative) in inventory {
            let path = relative_path(relative.as_str());
            let Some(target) = plan.destination_for(path) else { continue };
            let original = checked_path(&source, path)?;
            let checksum = match plan.transferred_checksum(path) {
                Some(checksum) => checksum,
                None => book_checksum(&original)?,
            };
            // A merged audiobook can acquire extra tracks. Let the scanner
            // identify that new content rather than marking its old revision available.
            if !db.book_has_content_revision(book, checksum)? || (original.is_dir() && book_checksum(&checked_path(&destination, target)?)? != checksum) {
                continue;
            }
            if book.content_hash().is_none() {
                destination_assets.remember_local_content(book, checksum)?;
            }
            relocated_files.push((relative.to_string(), stored_relative_path(target)?));
        }
        if !assets.root_available() {
            return Err("source storage changed during the move; original files were retained".into());
        }
        db.stage_native_root_move(source.to_string_lossy().as_ref(), destination.to_string_lossy().as_ref(), id, assets.native_storage_identity(), &relocated_files, &relocated_directories, &[])?;
        let cleanup = PendingCleanup { library: id, source_identity: active_identity, books: leases.keys().copied().collect(), plan };
        let entry = self.config.registry.commit_library_move(id, source, destination, cleanup.source_identity.clone(), serde_json::to_vec(&cleanup).map_err(BackendError::operation)?)?;
        transfer.commit();
        // Rebinding applies the staged database transition. Return the committed
        // path directly so a later registry read cannot fall back to the old root.
        Ok(entry.storage_locator().to_owned())
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct PendingCleanup {
    pub library: LibraryId,
    pub source_identity: String,
    pub books: Vec<BookId>,
    pub plan: MovePlan,
}

impl LibraryService {
    pub(super) fn start_library_move_cleanups(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        self.config.executor.spawn_detached(async move {
            loop {
                let Some(backend) = weak.upgrade() else { return };
                match backend.config.registry.library_move_cleanup_keys() {
                    Ok(pending) => {
                        for key in pending {
                            // Relocation cannot make this old root live while its
                            // snapshot is being retired. Book edits stay available.
                            let _location = backend.config.library_location_lock.lock().await;
                            let worker = backend.clone();
                            if let Err(error) = executor::run_blocking(move || worker.finish_library_move_cleanup(key)).await.map_err(BackendError::operation).and_then(|result| result) {
                                log::warn!("old library files will be cleaned up later: {error}");
                            }
                        }
                    }
                    Err(error) => log::warn!("could not read pending library cleanup: {error}"),
                }
                drop(backend);
                if executor::sleep(Duration::from_secs(30)).await.is_err() {
                    return;
                }
            }
        });
    }

    fn finish_library_move_cleanup(&self, key: String) -> Result<(), BackendError> {
        let Some(bytes) = self.config.registry.read_value(&key)? else { return Ok(()) };
        let mut cleanup: PendingCleanup = serde_json::from_slice(&bytes).map_err(BackendError::operation)?;
        let root = cleanup.plan.source.clone();
        // Reusing an old root, including moving back, supersedes its cleanup.
        if self.config.registry.entries()?.iter().any(|entry| {
            let live = Path::new(entry.storage_locator());
            live.starts_with(&root) || root.starts_with(live)
        }) {
            self.config.registry.remove_value(&key)?;
            return Ok(());
        }
        if !root.try_exists()? || !cleanup_root_matches(&root, &cleanup.source_identity, cleanup.library)? {
            return Ok(());
        }
        let source = AssetStore::open(root.to_string_lossy().as_ref())?;
        let mut leases = HashMap::new();
        if !lease_books(&source, cleanup.books.iter().copied(), &mut leases)? {
            return Ok(());
        }
        let mut checkpoint = cleanup.clone();
        cleanup.plan.cleanup_originals(
            || {
                if cleanup_root_matches(&root, &cleanup.source_identity, cleanup.library)? {
                    Ok(())
                } else {
                    Err(io::Error::other("old storage changed; original files were retained"))
                }
            },
            |plan| {
                checkpoint.plan = plan.clone();
                let bytes = serde_json::to_vec(&checkpoint).map_err(io::Error::other)?;
                self.config.registry.write_value(&key, &bytes).map_err(io::Error::other)
            },
        )?;
        self.config.registry.retire_library_move_source(cleanup.library, root, cleanup.source_identity)?;
        self.config.registry.remove_value(&key)?;
        Ok(())
    }
}
