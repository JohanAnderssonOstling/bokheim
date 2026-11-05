//! Filesystem discovery, inspection, and scan status for one library.

use crate::library::{LibraryHandle, LibraryResources};
use crate::BackendError;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use library_model::LibraryUpdate;
use std::sync::atomic::Ordering;

#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use crate::library::events::LibraryEvent;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use client_platform_runtime::task_scope::OnDrop;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use filesystem_scanner::filesystem_scan::{discover, inspect_files, ScanBookIndex, PUBLISH_BATCH_SIZE};
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use library_database::ContributorNameMatcher;

#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
pub(crate) enum FolderScan {
    Complete,
    Deferred,
}

impl LibraryHandle {
    pub async fn scanning(&self) -> Result<bool, BackendError> {
        let session = &self.owner.resources;
        Ok(session.scanner_running())
    }
}

impl LibraryResources {
    pub fn scanner_running(&self) -> bool {
        self.scanning.load(Ordering::Acquire)
    }

    /// Walks the library folder and applies what it finds, walking again while
    /// the database changed underneath. Errors and cancellation release the
    /// scanning flag too, so file replay can continue between scan attempts.
    #[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
    pub(crate) async fn scan_folder(&self) -> Result<FolderScan, BackendError> {
        let assets = self.state.read().await.assets().clone();
        if !assets.root_available() {
            return Err(BackendError::classified(client_protocol::Failure::LibraryFolderUnavailable, "library folder identity is unavailable"));
        }
        self.scanning.store(true, Ordering::Release);
        self.events.updates.publish(LibraryUpdate::Scanning(true));
        let scanning = OnDrop::new(|| {
            self.scanning.store(false, Ordering::Release);
            self.events.updates.publish(LibraryUpdate::Scanning(false));
            self.events.emit(LibraryEvent::ContentsChanged);
        });
        let outcome = loop {
            let snapshot = self.database.scan_snapshot().await?;
            let outcome = if snapshot.excluded_paths.is_empty() { FolderScan::Complete } else { FolderScan::Deferred };
            match futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(self.scan_once(snapshot))).await.unwrap_or_else(|_| Err(BackendError::message("library scan panicked")))? {
                library_database::ScanApplyResult::Applied => break outcome,
                // Coalesce changes while the owner keeps serving requests.
                library_database::ScanApplyResult::Stale => crate::executor::sleep(std::time::Duration::from_millis(250)).await.map_err(BackendError::operation)?,
            }
        };
        drop(scanning);
        // A completed scan can leave lifecycle and metadata mutations
        // that need a sync pass before dependent asset work is ready.
        self.events.sync.wake();
        Ok(outcome)
    }

    #[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
    async fn scan_once(&self, snapshot: library_database::ScanSnapshot) -> Result<library_database::ScanApplyResult, BackendError> {
        let assets = self.state.read().await.assets().clone();
        let root_access = assets.lock_root_shared().await;
        // Walking directories and fingerprinting files is blocking I/O.
        // Keep it off the library thread.
        let (mut snapshot, mut discovery) = crate::executor::run_blocking({
            let root = assets.native_library_root().to_owned();
            move || {
                let _root = root_access;
                let discovery = discover(root, &snapshot);
                (snapshot, discovery)
            }
        })
        .await
        .map_err(BackendError::operation)?;
        let mut readable = discovery.readable;
        let known = ScanBookIndex::new(&snapshot);
        for files in discovery.files.chunks(PUBLISH_BATCH_SIZE) {
            let inspected = inspect_files(assets.native_library_root(), files, discovery.readable, &snapshot, &known, &self.cpu).await;
            if !assets.root_available() {
                return Err(BackendError::message("library folder changed during scanning"));
            }
            readable &= inspected.readable;
            if inspected.books.is_empty() {
                tokio::task::yield_now().await;
                continue;
            }
            let mut contents = Vec::with_capacity(inspected.books.len());
            for book in &inspected.books {
                let checksum = book.book_id.content_hash().ok_or_else(|| BackendError::message("scanned bytes have no checksum"))?;
                let identity = self.database.resolve_scanned_content(checksum, book.source_identity).await?;
                contents.push((identity, checksum));
            }
            assets.remember_local_contents(contents).await?;
            let result = self
                .database
                .apply_filesystem_scan_batch(&mut snapshot, &discovery, library_database::InspectedFilesystemScan { readable, books: inspected.books }, false, Some(ContributorNameMatcher::Normalized))
                .await
                .map_err(BackendError::operation)?;
            if result == library_database::ScanApplyResult::Stale {
                return Ok(result);
            }
            discovery.directories.clear();
            self.events.emit(LibraryEvent::ContentsChanged);
            self.events.sync.wake();
            // Deliver queries and partial contents before inspecting another batch.
            tokio::task::yield_now().await;
        }
        if !assets.root_available() {
            return Err(BackendError::message("library folder changed during scanning"));
        }
        let result = self
            .database
            .apply_filesystem_scan_batch(&mut snapshot, &discovery, library_database::InspectedFilesystemScan { readable, books: vec![] }, true, Some(ContributorNameMatcher::Normalized))
            .await
            .map_err(BackendError::operation)?;
        if result == library_database::ScanApplyResult::Applied && !discovery.readable {
            return Err(BackendError::message("filesystem scan was incomplete"));
        }
        Ok(result)
    }
}
