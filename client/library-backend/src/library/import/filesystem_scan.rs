//! Filesystem discovery, inspection, and scan status for one library.

use crate::BackendError;
use crate::library::{LibraryHandle, LibraryResources};
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use library_model::LibraryUpdate;
use std::sync::atomic::Ordering;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use std::time::Instant;

#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use crate::library::events::LibraryEvent;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use client_platform_runtime::profiling::ProfileSpan;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use client_platform_runtime::task_scope::OnDrop;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use filesystem_scanner::filesystem_scan::{PUBLISH_BATCH_SIZE, ScanBookIndex, discover, inspect_files};
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use library_database::{ContributorNameMatcher, ScanApplyResult};

#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
struct ScanProfileStage {
    name: &'static str,
    files: usize,
    started: Option<Instant>,
    _detail: ProfileSpan,
}

#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
impl ScanProfileStage {
    fn new(name: &'static str, files: usize) -> Self {
        Self { name, files, started: log::log_enabled!(target: "library_add_profile", log::Level::Debug).then(Instant::now), _detail: ProfileSpan::new(name, "scan_phase") }
    }
}

#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
impl Drop for ScanProfileStage {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            log::debug!(target: "library_add_profile", "stage={} files={} elapsed_ms={:.3}", self.name, self.files, started.elapsed().as_secs_f64() * 1000.0);
        }
    }
}

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

    /// Completes one walk even when concurrent changes require a follow-up.
    /// Releasing the scanning flag lets file replay run before that next pass.
    #[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
    pub(crate) async fn scan_folder(&self) -> Result<FolderScan, BackendError> {
        let _profile = ScanProfileStage::new("scan", 0);
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
        let profile = ScanProfileStage::new("snapshot", 0);
        let snapshot = self.database.scan_snapshot(book_inspection::inspection_version(book_model::BookFormat::Pdf)).await?;
        drop(profile);
        let excluded = !snapshot.excluded_paths.is_empty();
        let result = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(self.scan_once(snapshot))).await.unwrap_or_else(|_| Err(BackendError::message("library scan panicked")))?;
        let outcome = if excluded || result == ScanApplyResult::AppliedNeedsRescan { FolderScan::Deferred } else { FolderScan::Complete };
        drop(scanning);
        // A completed scan can leave lifecycle and metadata mutations
        // that need a sync pass before dependent asset work is ready.
        self.events.sync.wake();
        Ok(outcome)
    }

    #[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
    async fn scan_once(&self, snapshot: library_database::ScanSnapshot) -> Result<ScanApplyResult, BackendError> {
        let assets = self.state.read().await.assets().clone();
        let root_access = assets.lock_root_shared().await;
        // Walking directories and fingerprinting files is blocking I/O.
        // Keep it off the library thread.
        let (mut snapshot, mut discovery) = crate::executor::run_blocking({
            let root = assets.native_library_root().to_owned();
            move || {
                let _profile = ScanProfileStage::new("discovery", 0);
                let _root = root_access;
                let discovery = discover(root, &snapshot);
                (snapshot, discovery)
            }
        })
        .await
        .map_err(BackendError::operation)?;
        let mut readable = discovery.readable;
        let mut needs_rescan = false;
        let known = ScanBookIndex::new(&snapshot);
        // Associate unchanged legacy audiobooks with their new fingerprints
        // before admitting new copies. Keep the two groups in separate batches
        // so identity lookups can see those committed aliases.
        discovery.files.sort_by_key(|file| !known.unchanged(&snapshot, file));
        let unchanged = discovery.files.partition_point(|file| known.unchanged(&snapshot, file));
        let batches = [&discovery.files[..unchanged], &discovery.files[unchanged..]].into_iter().flat_map(|files| files.chunks(PUBLISH_BATCH_SIZE));
        for files in batches {
            let profile = ScanProfileStage::new("inspection", files.len());
            let inspected = inspect_files(assets.native_library_root(), files, discovery.readable, &snapshot, &known, &self.cpu).await;
            drop(profile);
            if !assets.root_available() {
                return Err(BackendError::message("library folder changed during scanning"));
            }
            readable &= inspected.readable;
            if inspected.books.is_empty() {
                tokio::task::yield_now().await;
                continue;
            }
            let profile = ScanProfileStage::new("identity_lookup", inspected.books.len());
            let mut contents = Vec::with_capacity(inspected.books.len());
            for book in &inspected.books {
                let (identity, proof) = if let Some(checksum) = book.audiobook_content_hash {
                    let unchanged = snapshot.books.iter().find(|known| known.directory == book.directory && known.name == book.name && known.fingerprint == book.fingerprint).map(|known| known.book_id);
                    let known = match book.byte_checksum {
                        Some(bytes) => self.database.book_for_local_content(bytes).await?.or(unchanged),
                        None => unchanged,
                    };
                    (self.database.resolve_audiobook_content(checksum, known).await?, book.byte_checksum.unwrap_or(checksum))
                } else {
                    let checksum = book.byte_checksum.ok_or_else(|| BackendError::message("scanned bytes have no checksum"))?;
                    (self.database.resolve_scanned_content(checksum, book.source_identity).await?, checksum)
                };
                contents.push((identity, proof));
            }
            drop(profile);
            let profile = ScanProfileStage::new("checksum_persistence", inspected.books.len());
            assets.remember_local_contents(contents).await?;
            drop(profile);
            let profile = ScanProfileStage::new("commit", inspected.books.len());
            let result = self
                .database
                .apply_filesystem_scan_batch(&mut snapshot, &discovery, library_database::InspectedFilesystemScan { readable, books: inspected.books }, false, Some(ContributorNameMatcher::Normalized))
                .await
                .map_err(BackendError::operation)?;
            drop(profile);
            needs_rescan |= result == ScanApplyResult::AppliedNeedsRescan;
            discovery.directories.clear();
            self.events.emit(LibraryEvent::ContentsChanged);
            self.events.sync.wake();
            // Deliver queries and partial contents before inspecting another batch.
            tokio::task::yield_now().await;
        }
        if !assets.root_available() {
            return Err(BackendError::message("library folder changed during scanning"));
        }
        let _profile = ScanProfileStage::new("finalize", 0);
        let result = self
            .database
            .apply_filesystem_scan_batch(&mut snapshot, &discovery, library_database::InspectedFilesystemScan { readable, books: vec![] }, true, Some(ContributorNameMatcher::Normalized))
            .await
            .map_err(BackendError::operation)?;
        if !discovery.readable {
            return Err(BackendError::message("filesystem scan was incomplete"));
        }
        Ok(if needs_rescan { ScanApplyResult::AppliedNeedsRescan } else { result })
    }
}
