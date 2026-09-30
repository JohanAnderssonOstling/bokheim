use super::error::SyncResult;
use super::library_sync::LibrarySync;
use asset_transfer::transfer::TransferWorker;
use asset_transfer::{TransferJob, TransferOrigin};
use library_database::TransferOutcome;
use sync_common::ContentHash;

// A manifest is metadata only.  Use the server's contractual ceiling here;
// actual book-byte uploads stay constrained by `book_batch` limits.
const BOOK_MANIFEST_PAGE_ENTRIES: i64 = sync_common::api::assets::MAX_BLOB_MANIFEST_ENTRIES as i64;
const COVER_PRESENCE_PAGE_ENTRIES: i64 = sync_common::api::assets::MAX_THUMBNAIL_PRESENCE_ENTRIES as i64;
const THUMBNAIL_DOWNLOAD_PAGE_ENTRIES: usize = 32;

struct TransferPlanGuard(std::sync::Arc<asset_transfer::TransferQueue>, u64);
impl Drop for TransferPlanGuard {
    fn drop(&mut self) {
        self.0.cancel_reconcile(self.1);
    }
}

impl LibrarySync {
    /// Asks the server which live books have a cover. Covers are set once,
    /// so presence is all there is to learn. A cover the server has lost is
    /// uploaded again from any device that still holds it.
    pub(crate) async fn refresh_remote_cover_presence(&self) -> SyncResult<()> {
        if !self.is_signed_in() { return Ok(()); }
        let credentials = self.credentials.credentials().ok_or_else(|| super::error::SyncError::failed("missing credentials"))?;
        let mut after = String::new();
        loop {
            let hashes = self.database()?.remote_thumbnail_presence_candidates_page(&after)?;
            if hashes.is_empty() { break; }
            after = hashes.last().expect("nonempty cover page").as_str().to_owned();
            let response = sync_transport::thumbnail_presence(&self.http_client, &credentials, &self.library_id, hashes.clone()).await.map_err(super::error::SyncError::failed)?;
            let requested = hashes.into_iter().collect::<std::collections::HashSet<_>>();
            let present = response.present.iter().copied().collect::<std::collections::HashSet<_>>();
            if present.len() != response.present.len() || !present.is_subset(&requested) {
                return Err(super::error::SyncError::failed("invalid thumbnail presence response"));
            }
            let database = self.database()?;
            for hash in &present {
                database.record_remote_asset(library_replica::BlobKind::Thumbnail, hash)?;
            }
            for hash in requested.difference(&present) {
                database.forget_remote_asset(library_replica::BlobKind::Thumbnail, hash)?;
            }
        }
        Ok(())
    }

    pub(crate) async fn record_remote_thumbnail_presence(&self, hashes: Vec<ContentHash>) -> SyncResult<()> {
        if hashes.is_empty() || !self.is_signed_in() {
            return Ok(());
        }
        let credentials = self.credentials.credentials().ok_or_else(|| super::error::SyncError::failed("missing credentials"))?;
        let database = self.database()?;
        for page in hashes.chunks(COVER_PRESENCE_PAGE_ENTRIES as usize) {
            let response = sync_transport::thumbnail_presence(&self.http_client, &credentials, &self.library_id, page.to_vec()).await.map_err(super::error::SyncError::failed)?;
            for hash in response.present {
                database.record_remote_asset(library_replica::BlobKind::Thumbnail, &hash)?;
            }
        }
        Ok(())
    }
    /// Queues cover downloads a page at a time, driving each page before the next.
    async fn run_thumbnail_downloads(&self, worker: &TransferWorker, generation: u64, hashes: &[ContentHash]) -> SyncResult<()> {
        for page in hashes.chunks(THUMBNAIL_DOWNLOAD_PAGE_ENTRIES) {
            let jobs = page.iter().map(|hash| TransferJob::download_thumbnail(*hash, TransferOrigin::Background)).collect();
            self.transfer_queue.reconcile_page(generation, jobs);
            worker.drive().await;
        }
        Ok(())
    }

    pub(super) async fn sync_assets(&self, targeted: Vec<ContentHash>) -> SyncResult<()> {
        self.transfer_queue.resume_authentication();
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
        for hash in self.database()?.pending_download_requests()? {
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
                let intents = self.database()?.pending_book_uploads(after, BOOK_MANIFEST_PAGE_ENTRIES)?;
                if intents.is_empty() {
                    break;
                }
                after = intents.last().expect("non-empty page has a last intent").id;
                let mut jobs = Vec::new();
                for intent in intents {
                    let local = self.assets.exists(library_replica::BlobKind::Book, &intent.content_hash).map_err(super::error::SyncError::failed)?;
                    if local {
                        jobs.push(TransferJob::upload_book_intent(intent, false, TransferOrigin::Background));
                    }
                }
                // A prepared entry can still lose its local blob outside the
                // application. Skip that stale entry for this pass so it
                // cannot hide later transferable records behind the page.
                if jobs.is_empty() {
                    continue;
                }
                // Upload negotiation and retries belong to the background driver.
                // Planning must return even when the book endpoint is stalled.
                worker.run_page(generation, jobs);
                tokio::task::yield_now().await;
            }

            self.refresh_remote_cover_presence().await?;

            // Covers are derived directly from durable thumbnail and remote
            // presence state.
            loop {
                let hashes = self.database()?.pending_cover_uploads(COVER_PRESENCE_PAGE_ENTRIES)?;
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
        worker.finish_plan(generation);
        Ok(())
    }

    pub(super) fn prepare_requested_download(&self, hash: &ContentHash) -> SyncResult<Option<TransferJob>> {
        Ok(asset_transfer::planning::prepare_requested_download(&self.assets, &self.database()?, hash)?)
    }
}
