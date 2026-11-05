//! Explicit file-operation and thumbnail jobs, also available while signed out.
pub(crate) use super::LibraryResources;
pub(crate) use crate::library::events::LibraryEvent;
pub(crate) use crate::BackendError;
pub(crate) use crate::BookId;
use asset_transfer::thumbnail_jobs::{ThumbnailExtractor, ThumbnailJobs};

impl LibraryResources {
    pub(crate) async fn run_file_jobs(&self) -> Result<(), BackendError> {
        let database = &self.database;
        let had_file_work = database.has_pending_file_work().await?;
        let result = library_placement::run_file_jobs(&self.state).await;
        if had_file_work {
            // Replay completion refreshes views and sync, but is not another
            // request for replay. Durable work can remain queued (for example
            // while a book is leased); ContentsChanged would self-wake forever.
            self.events.refresh_contents();
            self.events.sync.wake();
        }
        result.map_err(BackendError::operation)?;
        // Provider cover generation and publication has its own revision-aware
        // worker. Ordinary thumbnail jobs cannot overwrite that cache.

        let scope = database.prepared_upload_batch().await?.hashes();
        if let Err(error) = self.transfers.record_remote_thumbnail_presence(scope.iter().copied().collect()).await {
            log::warn!("remote thumbnail presence will retry: {error}");
        }

        Ok(())
    }

    pub(crate) async fn run_thumbnail_jobs(&self) -> Result<(), BackendError> {
        let hashes = self.database.prepared_upload_thumbnail_candidates().await?;
        self.run_thumbnail_members(hashes).await
    }

    pub(crate) async fn run_thumbnail_members(&self, hashes: Vec<BookId>) -> Result<(), BackendError> {
        let extractor = ThumbnailExtractor::new(self.cpu.clone());
        let mut failure = None;
        for hash in hashes {
            let result = async {
                let assets = self.state.read().await.assets().clone();
                let prepared = ThumbnailJobs::new(&self.database, &assets, &extractor).prepare(hash).await?;
                if let Some(job) = prepared {
                    let events = self.events.clone();
                    job.run(&extractor, move |has_cover| {
                        events.emit(LibraryEvent::ThumbnailFinished { book_id: hash, result: Ok(has_cover) });
                    })
                    .await?;
                } else {
                    self.events.sync.wake();
                    self.events.enrichment.wake();
                }
                Ok::<_, BackendError>(())
            }
            .await;
            if let Err(error) = result {
                log::warn!("thumbnail recovery for {hash} will retry: {error}");
                self.events.emit(LibraryEvent::ThumbnailFinished { book_id: hash, result: Err(error.failure()) });
                failure.get_or_insert(error);
            }
            tokio::task::yield_now().await;
        }
        failure.map_or(Ok(()), Err)
    }
}
