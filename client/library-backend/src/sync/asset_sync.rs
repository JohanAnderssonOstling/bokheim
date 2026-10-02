use std::collections::HashSet;
pub(crate) use super::error::SyncResult;
pub(crate) use super::library_sync::LibrarySync;
use crate::sync::error::SyncError;
pub(crate) use asset_transfer::transfer::TransferWorker;
pub(crate) use asset_transfer::TransferJob;
pub(crate) use library_database::TransferOutcome;
pub(crate) use library_model::TransferOrigin;
pub(crate) use sync_common::ContentHash;

// A manifest is metadata only.  Use the server's contractual ceiling here;
// actual book bytes use bounded concurrent individual PUTs.
pub(crate) const BOOK_MANIFEST_PAGE_ENTRIES: i64 = sync_common::api::assets::MAX_BLOB_MANIFEST_ENTRIES as i64;
pub(crate) const COVER_PRESENCE_PAGE_ENTRIES: i64 = sync_common::api::assets::MAX_THUMBNAIL_PRESENCE_ENTRIES as i64;
pub(crate) const THUMBNAIL_DOWNLOAD_PAGE_ENTRIES: usize = 32;

pub(crate) struct TransferPlanGuard(pub(crate) std::sync::Arc<asset_transfer::TransferQueue>, pub(crate) u64);
impl Drop for TransferPlanGuard {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.generation != self.1 || !state.planning {
            return;
        }
        state.planning = false;
        self.0.finalize_empty_batches(&mut state);
    }
}

impl LibrarySync {
    /// Records which of these books already have a cover on the server.
    pub(crate) async fn record_remote_thumbnail_presence(&self, hashes: Vec<ContentHash>) -> SyncResult<()> {
        if hashes.is_empty() || !self.is_signed_in() {
            return Ok(());
        }
        let credentials = self.credentials.credentials().ok_or_else(|| super::error::SyncError::failed("missing credentials"))?;
        let database = self.database()?;
        for page in hashes.chunks(COVER_PRESENCE_PAGE_ENTRIES as usize) {
            let response = sync_transport::thumbnail_presence(&self.http_client, &credentials, &self.library_id, page.to_vec()).await.map_err(super::error::SyncError::failed)?;
            for hash in response.present {
                database.record_remote_asset(library_model::BlobKind::Thumbnail, &hash)?;
            }
        }
        Ok(())
    }

    /// Queues cover downloads a page at a time, driving each page before the next.
    pub(crate) async fn run_thumbnail_downloads(&self, worker: &TransferWorker, generation: u64, hashes: &[ContentHash]) -> SyncResult<()> {
        for page in hashes.chunks(THUMBNAIL_DOWNLOAD_PAGE_ENTRIES) {
            let jobs = page.iter().map(|hash| TransferJob { operation: library_runtime::TransferOperation::DownloadThumbnail { thumbnail_hash: *hash }, origin: TransferOrigin::Background }).collect();
            self.transfer_queue.reconcile_page(generation, jobs);
            worker.drive().await;
        }
        Ok(())
    }

    pub(crate) fn prepare_requested_download(&self, hash: &ContentHash) -> SyncResult<Option<TransferJob>> {
        Ok(asset_transfer::planning::prepare_requested_download(&self.assets, &self.database()?, hash)?)
    }
}

impl LibrarySync {
    pub(crate) async fn reconcile_assets(&self, targeted: Vec<ContentHash>) -> SyncResult<()> {
        use super::library_sync::SyncActivity;
        let _asset_guard = self.asset_gate.lock().await;
        if !self.is_signed_in() {
            return Ok(());
        }
        let _activity = SyncActivity::begin(&self.asset_activity, self.event_tx.as_ref());

        use crate::sync::asset_sync::*;

        self.transfer_queue.authentication_paused.store(false, std::sync::atomic::Ordering::Release);
        self.transfer_queue.wake_scheduler.notify_one();
        let worker = self.transfer_worker();
        let generation = self.transfer_queue.begin_reconcile();
        let _plan = TransferPlanGuard(self.transfer_queue.clone(), generation);
        // Covers for payload-woken books jump the lanes; everything else
        // follows below in the usual order.
        if !targeted.is_empty() {
            let hashes = self.database()?.pending_thumbnail_downloads(Some(&targeted))?;
            self.run_thumbnail_downloads(&worker, generation, &hashes).await?;
        }
        // Requested downloads run first: user-facing work jumps the bulk queue.
        let mut jobs = Vec::new();
        for hash in {
            use library_database::assets::*;
            use library_database::shared_sql::*;
            use library_database::*;
            use library_model::{BookPlacement, BookUploadIntent, RelativeBookPath};
            use sync_common::ContentHash;
            let database = self.database()?;

            database.book_download_requests()?.into_iter().map(|(hash, _)| hash).collect::<Vec<_>>()
        } {
            jobs.extend(self.prepare_requested_download(&hash)?);
        }
        if !jobs.is_empty() {
            self.transfer_queue.reconcile_page(generation, jobs);
            worker.drive().await;
        }
        // Book uploads read the intent queue. Drain successive bounded
        // negotiation pages here:
        // server-present books are settled without a byte transfer and must
        // not wait for the background scheduler's retry interval.
        if self.asset_storage_enabled() {
            let mut after = 0;
            loop {
                // One connection per page, released before the page is handed
                // to the transfer queue.
                let database = self.database()?;
                let intents = {
                    use library_database::assets::*;
                    use library_database::shared_sql::*;
                    use library_database::*;
                    use library_model::{BookPlacement, BookUploadIntent, RelativeBookPath};
                    use sync_common::ContentHash;
                    let after = after;
                    let limit = BOOK_MANIFEST_PAGE_ENTRIES;

                    let mut intents = Vec::new();
                    database.connection.transfer_pending_book_uploads(after, limit, library_database::assets::rejection_cutoff()?, |row| {
                        intents.push(BookUploadIntent { id: row.get(0)?, content_hash: ContentHash::new(&row.get::<_, String>(1)?), checksum: ContentHash::new(&row.get::<_, String>(2)?), size_bytes: row.get::<_, i64>(3)? as u64 });
                        Ok(())
                    })?;
                    intents
                };
                if intents.is_empty() {
                    break;
                }
                after = intents.last().expect("non-empty page has a last intent").id;
                let mut jobs = Vec::new();
                for intent in intents {
                    let local = self.assets.book_exists(&intent.content_hash, &database.book_paths(&intent.content_hash).map_err(SyncError::failed)?).map_err(SyncError::failed)?;
                    if local {
                        jobs.push(TransferJob::upload_book_intent(intent, false, TransferOrigin::Background));
                    }
                }
                drop(database);
                // A prepared entry can still lose its local blob outside the
                // application. Skip that stale entry for this pass so it
                // cannot hide later transferable records behind the page.
                if jobs.is_empty() {
                    continue;
                }
                // Upload negotiation and retries belong to the background driver.
                // Planning must return even when the book endpoint is stalled.
                worker.queue.reconcile_page(generation, jobs);
                asset_transfer::transfer::TransferWorker::ensure_driver(worker.clone());
                tokio::task::yield_now().await;
            }

            if self.is_signed_in() {
                let credentials = self.credentials.credentials().ok_or_else(|| SyncError::failed("missing credentials"))?;
                let mut after = String::new();
                loop {
                    let hashes = {
                        use library_database::enrichment::EnrichmentSql;
                        let database = self.database()?;
                        let mut hashes = Vec::new();
                        database
                            .connection
                            .thumbnail_remote_presence_candidates(&after, |row| {
                                hashes.push(ContentHash::new(&row.get::<_, String>(0)?));
                                Ok(())
                            })
                            .map_err(library_database::DatabaseError::from)?;
                        hashes
                    };
                    if hashes.is_empty() {
                        break;
                    }
                    after = hashes.last().expect("nonempty cover page").as_str().to_owned();
                    let response = sync_transport::thumbnail_presence(&self.http_client, &credentials, &self.library_id, hashes.clone()).await.map_err(SyncError::failed)?;
                    let requested = hashes.into_iter().collect::<HashSet<_>>();
                    let present = response.present.iter().copied().collect::<HashSet<_>>();
                    if present.len() != response.present.len() || !present.is_subset(&requested) {
                        return Err(SyncError::failed("invalid thumbnail presence response").into());
                    }
                    let database = self.database()?;
                    for hash in &present {
                        database.record_remote_asset(library_model::BlobKind::Thumbnail, hash)?;
                    }
                    for hash in requested.difference(&present) {
                        database.forget_remote_asset(library_model::BlobKind::Thumbnail, hash)?;
                    }
                }
            };

            // Covers are derived directly from durable thumbnail and remote
            // presence state.
            loop {
                let hashes = {
                    use library_database::assets::*;
                    use library_database::shared_sql::*;
                    use library_database::*;
                    use library_model::{BookPlacement, BookUploadIntent, RelativeBookPath};
                    use sync_common::ContentHash;
                    let database = self.database()?;
                    let limit = COVER_PRESENCE_PAGE_ENTRIES;

                    let mut hashes = Vec::new();
                    database.connection.transfer_pending_cover_uploads(limit, library_database::assets::rejection_cutoff()?, |row| {
                        hashes.push(ContentHash::new(&row.get::<_, String>(0)?));
                        Ok(())
                    })?;
                    hashes
                };
                if hashes.is_empty() {
                    break;
                }
                let database = self.database()?;
                let snapshot = database.transfer_snapshot(&hashes)?;
                let mut outcome = TransferOutcome::default();
                let jobs = worker.prepare_thumbnail_uploads_with_refresh(&snapshot, &hashes, &mut outcome).await?;
                database.commit_transfer_outcome(&outcome)?;
                if !jobs.is_empty() {
                    self.transfer_queue.reconcile_page(generation, jobs);
                    worker.drive().await;
                }
            }
        }
        let hashes = self.database()?.pending_thumbnail_downloads(None)?;
        self.run_thumbnail_downloads(&worker, generation, &hashes).await?;
        worker.queue.finish_reconcile(generation);
        asset_transfer::transfer::TransferWorker::ensure_driver(worker.clone());
        Ok(())
    }
}
