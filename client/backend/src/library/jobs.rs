//! Explicit file-operation and thumbnail jobs, also available while signed out.
use super::LibrarySession;
use crate::BackendError;
use crate::ContentHash;
use crate::library::events::LibraryEvent;

impl LibrarySession {
    pub(crate) fn pending_local_jobs(&self) -> Result<bool, BackendError> {
        Ok(self.pending_file_jobs()? || self.pending_thumbnail_jobs()?)
    }

    pub(crate) fn pending_thumbnail_jobs(&self) -> Result<bool, BackendError> {
        Ok(client_library::thumbnail_jobs::has_pending_thumbnail_work(&*self.db.get()?)?)
    }

    pub(crate) fn pending_file_jobs(&self) -> Result<bool, BackendError> {
        let conn = self.db.get()?;
        if self.assets.placement_work(&conn)?.0 {
            return Ok(true);
        }
        Ok(client_library::file_work::has_pending_purge_work(&conn)?)
    }

    #[cfg(any(test, feature = "storage-contract-tests"))]
    pub(crate) async fn run_local_jobs(&self) -> Result<(), BackendError> {
        let files = self.run_file_jobs().await;
        let covers = self.run_thumbnail_jobs(|| {}).await;
        files.and(covers)
    }

    pub(crate) async fn run_file_jobs(&self) -> Result<(), BackendError> {
        let had_file_work = {
            let conn = self.db.get()?;
            self.assets.placement_work(&conn)?.0
        };
        let had_purge_work = client_library::file_work::has_pending_purge_work(&*self.db.get()?)?;
        let result = self.assets.run_file_jobs(&self.db).await;
        if had_purge_work {
            self.emit(LibraryEvent::StorageChanged);
        }
        if had_file_work {
            self.emit(LibraryEvent::TransferStatusChanged);
            self.notify_contents_changed();
            self.notify_storage_changed();
        }
        result.map_err(BackendError::operation)
    }

    pub(crate) async fn run_thumbnail_jobs(&self, cover_inspected: impl Fn() + crate::executor::BackendSend) -> Result<(), BackendError> {
        self.run_thumbnail_batch(cover_inspected, &crate::worker_cpu::ThumbnailExtractor::default()).await
    }

    pub(crate) async fn run_thumbnail_batch(&self, cover_inspected: impl Fn() + crate::executor::BackendSend, extractor: &crate::worker_cpu::ThumbnailExtractor) -> Result<(), BackendError> {
        library_files::thumbnail_jobs::ThumbnailJobs::new(&self.db, &self.assets, extractor).run_batch(&mut ThumbnailEvents { library: self, cover_inspected }).await.map_err(BackendError::operation)
    }

    /// One activity spans the fixed preparation batch, including progress
    /// notifications. Failed members remain retryable without stopping peers.
    pub(crate) async fn run_thumbnail_members(&self, hashes: Vec<ContentHash>) -> Result<(), BackendError> {
        use library_files::thumbnail_jobs::ThumbnailObserver;
        if hashes.is_empty() {
            return Ok(());
        }
        self.prepare_thumbnail_batch(hashes.len() as u64);
        let _activity = self.begin_thumbnail_generation();
        let extractor = crate::worker_cpu::ThumbnailExtractor::default();
        let jobs = library_files::thumbnail_jobs::ThumbnailJobs::new(&self.db, &self.assets, &extractor);
        let mut observer = ThumbnailEvents { library: self, cover_inspected: || {} };
        let mut failure = None;
        for hash in hashes {
            match jobs.recover(hash, &mut observer).await {
                Ok(true) => observer.completed(),
                Ok(false) => {}
                Err(error) => {
                    log::warn!("file inspection for {hash} will retry: {error}");
                    failure.get_or_insert_with(|| BackendError::operation(error));
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }

    pub(super) async fn recover_thumbnail(&self, hash: ContentHash) -> Result<bool, BackendError> {
        library_files::thumbnail_jobs::ThumbnailJobs::new(&self.db, &self.assets, &crate::worker_cpu::ThumbnailExtractor::default())
            .recover(hash, &mut ThumbnailEvents { library: self, cover_inspected: || {} })
            .await
            .map_err(BackendError::operation)
    }

    pub(crate) async fn store_thumbnail(&self, hash: ContentHash, versions: &crate::worker_cpu::ThumbnailBytes) -> Result<(), BackendError> {
        library_files::thumbnail_jobs::store_thumbnail(&self.assets, hash, &versions.browse, &versions.high_density).await.map_err(BackendError::operation)?;
        self.emit(LibraryEvent::ThumbnailGenerated { content_hash: hash });
        Ok(())
    }
}

struct ThumbnailEvents<'a, F> {
    library: &'a LibrarySession,
    cover_inspected: F,
}
impl<F: Fn() + crate::executor::BackendSend> library_files::thumbnail_jobs::ThumbnailObserver for ThumbnailEvents<'_, F> {
    type Activity = library_runtime::session::ThumbnailActivity;
    fn prepare(&mut self, pending: u64) {
        self.library.prepare_thumbnail_batch(pending);
    }
    fn begin(&mut self) -> Self::Activity {
        self.library.begin_thumbnail_generation()
    }
    fn completed(&mut self) {
        self.library.complete_thumbnail_batch_item();
        (self.cover_inspected)();
    }
    fn ready(&mut self, content_hash: ContentHash) {
        self.library.emit(LibraryEvent::ThumbnailGenerated { content_hash });
    }
}
