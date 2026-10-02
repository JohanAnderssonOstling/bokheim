//! Explicit file-operation and thumbnail jobs, also available while signed out.
use super::session::ThumbnailActivity;
pub(crate) use super::LibrarySession;
pub(crate) use crate::library::events::LibraryEvent;
pub(crate) use crate::BackendError;
pub(crate) use crate::ContentHash;
use library_model::ScanProgress;

impl LibrarySession {
    pub(crate) async fn run_file_jobs(&self) -> Result<(), BackendError> {
        let had_file_work = self.db.has_pending_file_work()?;
        let had_purge_work = self.db.has_pending_purge_work()?;
        let result = crate::asset_workflow::run_file_jobs(&self.assets, &self.db).await;
        if had_purge_work {
            self.emit(LibraryEvent::StorageChanged);
        }
        if had_file_work {
            self.emit(LibraryEvent::TransferStatusChanged);
            // Replay completion refreshes views and sync, but is not another
            // request for replay. Durable work can remain queued (for example
            // while a book is leased); ContentsChanged would self-wake forever.
            if !self.runtime.has_directory_imports() {
                self.subscriptions.publish_update(library_model::LibraryUpdate::Contents);
                self.sync_wake.wake();
            }
            self.notify_storage_changed();
        }
        result.map_err(BackendError::operation)?;
        let snapshot = self.db.upload_preparation_snapshot()?;
        let scope = self.db.prepared_upload_batch(&snapshot.hashes())?.hashes();
        let sync = self.sync.clone();
        if let Err(error) = sync.record_remote_thumbnail_presence(scope.iter().copied().collect()).await {
            log::warn!("remote thumbnail presence will retry: {error}");
        }

        if self.assets.local_access_enabled() {
            // Freeze membership before preparation. Newly discovered files wait
            // for the next batch, including files added during slow inspection.
            let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
            let thumbnails = self.db.upload_thumbnail_candidates(&scope, now).map_err(BackendError::operation)?;
            let _thumbnails = if thumbnails.is_empty() { None } else { Some(self.runtime.begin_thumbnail_work(self.event_tx.clone()).ok_or_else(|| BackendError::message("local inspection is already running"))?) };
            // finish_file_work immediately admits and wakes enrichment, including
            // covers that extraction just classified as no_cover.
            self.run_thumbnail_members(thumbnails).await?;
        }
        Ok(())
    }

    pub(crate) async fn run_thumbnail_members(&self, hashes: Vec<ContentHash>) -> Result<(), BackendError> {
        use asset_transfer::thumbnail_jobs::{BatchSelection, ThumbnailExtractor, ThumbnailJobs, ThumbnailObserver};
        if hashes.is_empty() {
            return Ok(());
        }
        let extractor = ThumbnailExtractor::new(self.cpu.clone());
        let jobs = ThumbnailJobs::new(&self.db, &self.assets, &extractor);
        let mut observer = ThumbnailEvents { library: self, cover_inspected: || {} };
        observer.prepare(hashes.len() as u64);
        jobs.run_selected(BatchSelection::Members(hashes), &mut observer).await.map_err(BackendError::operation)
    }

    pub(crate) async fn store_thumbnail(&self, hash: ContentHash, versions: &cpu_host::ThumbnailBytes) -> Result<(), BackendError> {
        asset_transfer::thumbnail_jobs::store_thumbnail(&self.assets, hash, &versions.browse, &versions.high_density).await.map_err(BackendError::operation)?;
        self.emit(LibraryEvent::ThumbnailGenerated { content_hash: hash });
        Ok(())
    }
}

pub(crate) struct ThumbnailEvents<'a, F> {
    pub(crate) library: &'a LibrarySession,
    pub(crate) cover_inspected: F,
}
impl<F: Fn() + crate::executor::BackendSend> asset_transfer::thumbnail_jobs::ThumbnailObserver for ThumbnailEvents<'_, F> {
    type Activity = super::session::ThumbnailActivity;
    fn prepare(&mut self, pending: u64) {
        let mut state = self.library.runtime.thumbnails.state.lock().unwrap_or_else(|error| error.into_inner());
        if !state.batch_active && state.generations_running == 0 {
            state.progress = ScanProgress::default();
        }
        state.progress.total = state.progress.total.max(state.progress.succeeded + pending);
    }
    fn begin(&mut self) -> Self::Activity {
        self.library.runtime.begin_thumbnail_generation(self.library.event_tx.clone())
    }
    fn completed(&mut self) {
        {
            let mut state = self.library.runtime.thumbnails.state.lock().unwrap_or_else(|error| error.into_inner());
            state.progress.succeeded += 1;
            state.progress.total = state.progress.total.max(state.progress.succeeded);
        }
        self.library.emit(LibraryEvent::TransferStatusChanged);
        self.library.sync_wake.wake();
        (self.cover_inspected)();
    }
    fn ready(&mut self, content_hash: ContentHash) {
        self.library.emit(LibraryEvent::ThumbnailGenerated { content_hash });
    }
}
