//! Explicit file-operation and thumbnail jobs, also available while signed out.
pub(crate) use super::LibrarySession;
pub(crate) use crate::library::events::LibraryEvent;
pub(crate) use crate::BackendError;
pub(crate) use crate::ContentHash;

impl LibrarySession {
    pub(crate) async fn run_file_jobs(&self) -> Result<(), BackendError> {
        let had_file_work = self.db.has_pending_file_work()?;
        let result = library_placement::run_file_jobs(&self.assets, &self.db).await;
        if had_file_work {
            // Replay completion refreshes views and sync, but is not another
            // request for replay. Durable work can remain queued (for example
            // while a book is leased); ContentsChanged would self-wake forever.
            if !self.directory_imports_active() {
                self.events.publish_update(library_model::LibraryUpdate::Contents);
                self.events.sync.wake();
            }
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
            let thumbnails = self.db.upload_thumbnail_candidates(&scope).map_err(BackendError::operation)?;
            let _thumbnails = if thumbnails.is_empty() { None } else { Some(self.runtime.begin_thumbnail_work().ok_or_else(|| BackendError::message("local inspection is already running"))?) };
            // finish_file_work immediately admits and wakes enrichment, including
            // covers that extraction just classified as no_cover.
            self.run_thumbnail_members(thumbnails).await?;
        }
        Ok(())
    }

    pub(crate) async fn run_thumbnail_members(&self, hashes: Vec<ContentHash>) -> Result<(), BackendError> {
        use asset_transfer::thumbnail_jobs::{ThumbnailExtractor, ThumbnailJobs};
        if hashes.is_empty() {
            return Ok(());
        }
        let extractor = ThumbnailExtractor::new(self.cpu.clone());
        let jobs = ThumbnailJobs::new(&self.db, &self.assets, &extractor);
        let mut observer = ThumbnailEvents { library: self };
        jobs.run(hashes, &mut observer).await.map_err(BackendError::operation)
    }
}

pub(crate) struct ThumbnailEvents<'a> {
    pub(crate) library: &'a LibrarySession,
}
impl asset_transfer::thumbnail_jobs::ThumbnailObserver for ThumbnailEvents<'_> {
    fn completed(&mut self) {
        self.library.events.sync.wake();
    }
    fn ready(&mut self, content_hash: ContentHash) {
        self.library.emit(LibraryEvent::ThumbnailGenerated { content_hash });
    }
}
