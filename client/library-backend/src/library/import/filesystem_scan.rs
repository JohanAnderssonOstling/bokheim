//! Filesystem discovery, inspection, and scan status for one library.

use crate::library::{LibraryHandle, LibraryResources};
use crate::BackendError;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use library_model::LibraryUpdate;
use std::sync::atomic::Ordering;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use std::sync::Arc;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use std::time::Instant;

#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use book_model::BookFormat;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use client_platform_runtime::profiling::ProfileSpan;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use client_platform_runtime::task_scope::OnDrop;
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use filesystem_scanner::filesystem_scan::{discover, inspect_files, ScanBookIndex, PUBLISH_BATCH_SIZE};
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use library_database::{ContributorNameMatcher, ScanApplyResult};
#[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
use tokio::{sync::oneshot, task::JoinSet};

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
    /// Concurrent edits made observations stale; retry promptly.
    ChangedDuringScan,
    /// Pending file work excluded paths; wait for progress or a slow retry.
    Deferred,
    /// Discovery or inspection failed; retry without reporting a completed scan.
    Incomplete,
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
    /// Scanning activity is separate from committed contents notifications.
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
        });
        let profile = ScanProfileStage::new("snapshot", 0);
        let snapshot = self.database.scan_snapshot(book_inspection::inspection_version(book_model::BookFormat::Pdf)).await?;
        drop(profile);
        let excluded = !snapshot.excluded_paths.is_empty();
        let (result, readable) = futures_util::FutureExt::catch_unwind(std::panic::AssertUnwindSafe(self.scan_once(snapshot))).await.unwrap_or_else(|_| Err(BackendError::message("library scan panicked")))?;
        let outcome = if !readable {
            FolderScan::Incomplete
        } else if result == ScanApplyResult::AppliedNeedsRescan {
            FolderScan::ChangedDuringScan
        } else if excluded {
            FolderScan::Deferred
        } else {
            FolderScan::Complete
        };
        drop(scanning);
        Ok(outcome)
    }

    #[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
    async fn scan_once(&self, snapshot: library_database::ScanSnapshot) -> Result<(ScanApplyResult, bool), BackendError> {
        let mut contents_revision = self.database.scan_content_revision().await?;
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
        let known = Arc::new(ScanBookIndex::new(&snapshot));
        // Associate unchanged legacy audiobooks with their new fingerprints
        // before admitting new copies. Keep the two groups in separate batches
        // so identity lookups can see those committed aliases.
        discovery.files.sort_by_key(|file| !known.unchanged(&snapshot, file));
        let unchanged = discovery.files.partition_point(|file| known.unchanged(&snapshot, file));
        // Start PDFs and audiobook hashing ahead of publication so they overlap
        // across batch boundaries instead of waiting for each other. The inventory is
        // immutable; publication continues to refresh its own placement guards.
        let inspection_snapshot = Arc::new(snapshot.clone());
        let mut inspection_tasks = JoinSet::new();
        let batches = [&discovery.files[..unchanged], &discovery.files[unchanged..]]
            .into_iter()
            .flat_map(|files| files.chunks(PUBLISH_BATCH_SIZE))
            .map(|files| {
                let (ahead, others): (Vec<_>, Vec<_>) = files.iter().cloned().partition(|file| matches!(BookFormat::from_file_name(&file.name), Some(BookFormat::Pdf | BookFormat::M4b)) || file.directory_entry);
                let pending = if ahead.is_empty() {
                    None
                } else {
                    let snapshot = inspection_snapshot.clone();
                    let known = known.clone();
                    let cpu = self.cpu.clone();
                    let root = assets.native_library_root().to_owned();
                    let readable = discovery.readable;
                    let (sender, receiver) = oneshot::channel();
                    inspection_tasks.spawn(async move {
                        let inspected = inspect_files(root, &ahead, readable, &snapshot, &known, &cpu).await;
                        let _ = sender.send(inspected);
                    });
                    Some(receiver)
                };
                (files, others, pending)
            })
            .collect::<Vec<_>>();
        for (files, others, pending) in batches {
            let profile = ScanProfileStage::new("inspection", files.len());
            let mut inspected = inspect_files(assets.native_library_root(), &others, discovery.readable, &snapshot, &known, &self.cpu).await;
            if let Some(pending) = pending {
                let ahead = pending.await.map_err(BackendError::operation)?;
                inspected.readable &= ahead.readable;
                inspected.books.extend(ahead.books);
                inspected.books.sort_by_key(|book| files.iter().position(|file| file.directory == book.directory && file.name == book.name));
            }
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
            let prepared = self.database.prepare_filesystem_scan_batch(&snapshot.books, library_database::InspectedFilesystemScan { readable, books: inspected.books }).await.map_err(BackendError::operation)?;
            let contents = prepared.local_contents();
            let book_count = contents.len();
            drop(profile);
            let profile = ScanProfileStage::new("checksum_persistence", book_count);
            assets.remember_local_contents(contents).await?;
            drop(profile);
            let profile = ScanProfileStage::new("commit", book_count);
            let result = self.database.apply_prepared_filesystem_scan_batch(&mut snapshot, &discovery, prepared, false, Some(ContributorNameMatcher::Normalized)).await.map_err(BackendError::operation)?;
            drop(profile);
            needs_rescan |= result == ScanApplyResult::AppliedNeedsRescan;
            discovery.directories.clear();
            self.publish_scan_changes(&mut contents_revision).await?;
            // Deliver queries and partial contents between publication batches.
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
        self.publish_scan_changes(&mut contents_revision).await?;
        Ok((if needs_rescan { ScanApplyResult::AppliedNeedsRescan } else { result }, readable))
    }

    #[cfg(all(feature = "scanner", not(target_arch = "wasm32")))]
    async fn publish_scan_changes(&self, previous: &mut i64) -> Result<(), BackendError> {
        let current = self.database.scan_content_revision().await?;
        if current != *previous {
            *previous = current;
            self.events.notify_contents_changed();
        }
        Ok(())
    }
}
