use super::error::SyncResult;
use super::library_sync::LibrarySync;
use super::transfer::{TransferJob, TransferOrigin, TransferWorker};
use library_database::TransferOutcome;
use library_files::asset_store::AssetStore;
use sync_common::ContentHash;

struct TransferPlanGuard(std::sync::Arc<asset_transfer::TransferQueue>, u64);
impl Drop for TransferPlanGuard {
    fn drop(&mut self) {
        self.0.cancel_reconcile(self.1);
    }
}

use asset_transfer::planning::{AssetPlan, PlanCursor};

impl LibrarySync {
    async fn run_thumbnail_plan(&self, worker: &TransferWorker, generation: u64, plan: &AssetPlan) -> SyncResult<()> {
        let mut jobs = Vec::new();
        for hashes in plan.thumbnail_uploads.chunks(sync_common::api::assets::MAX_THUMBNAIL_BATCH_ENTRIES) {
            // Planning negotiates over a snapshot and hands the outcome back;
            // the session commits before those jobs reach the queue.
            let database = self.database()?;
            let snapshot = database.transfer_snapshot(hashes)?;
            let mut outcome = TransferOutcome::default();
            jobs.extend(worker.prepare_thumbnail_uploads_with_refresh(&snapshot, hashes, &mut outcome).await?);
            database.commit_transfer_outcome(&outcome)?;
        }
        jobs.extend(plan.thumbnail_downloads.iter().copied().map(|hash| TransferJob::download_thumbnail(hash, TransferOrigin::Background)));
        if !jobs.is_empty() {
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
        // Payload-targeted rows jump the lanes: plan exactly the woken
        // hashes through the shared inspect path, then fall through to
        // lanes and drain as usual.
        if !targeted.is_empty() {
            let rows = AssetPlan::read_rows(&self.database()?, &targeted)?;
            if !rows.is_empty() {
                let (plan, settled) = AssetPlan::inspect_rows(&self.assets, rows).await?;
                if !settled.is_empty() {
                    self.database()?.settle_asset_candidates(&settled)?;
                }
                self.run_thumbnail_plan(&worker, generation, &plan).await?;
            }
        }
        // Requested downloads run first: user-facing work jumps the bulk
        // queue. The request queue is read directly instead of deriving
        // from the asset_work drain below.
        let mut jobs = Vec::new();
        for hash in self.database()?.pending_download_requests()? {
            jobs.extend(self.prepare_requested_download(&hash)?);
        }
        if !jobs.is_empty() {
            self.transfer_queue.reconcile_page(generation, jobs);
            worker.drive().await;
        }
        // Book uploads read the intent queue directly instead of deriving
        // from the asset_work drain below, which now handles thumbnails
        // and file work. Bounded per pass like a plan page; upload
        // completion wakes the next pass for the remainder.
        if self.asset_storage_enabled() {
            let intents = self.database()?.pending_book_uploads(32)?;
            let requested: Vec<_> = intents.iter().map(|intent| intent.content_hash).collect();
            let ready = self.database()?.ready_upload_hashes(&requested)?;
            let mut jobs = Vec::new();
            for intent in intents {
                let local = self.assets.exists(library_replica::BlobKind::Book, &intent.content_hash).map_err(super::error::SyncError::failed)?;
                if ready.contains(&intent.content_hash) && local {
                    jobs.push(TransferJob::upload_book_intent(intent, false, TransferOrigin::Background));
                }
            }
            if !jobs.is_empty() {
                self.transfer_queue.reconcile_page(generation, jobs);
                worker.drive().await;
            }
        }
        let mut cursor = PlanCursor::default();
        // Each page is read synchronously, inspected with no database
        // reference held across the inspection's blocking-pool hop, and
        // committed synchronously again before the next page is read.
        while let Some(page) = AssetPlan::read_page(&self.database()?, &cursor)? {
            let (mut page, settled) = AssetPlan::inspect_page(&self.assets, page, &mut cursor).await?;
            if !settled.is_empty() {
                self.database()?.settle_asset_candidates(&settled)?;
            }
            let requested: Vec<_> = page.thumbnail_uploads.iter().copied().collect();
            let ready = self.database()?.ready_upload_hashes(&requested)?;
            page.thumbnail_uploads.retain(|hash| ready.contains(hash));
            self.run_thumbnail_plan(&worker, generation, &page).await?;
        }
        self.transfer_queue.finish_reconcile(generation);
        worker.drive().await;
        Ok(())
    }

    pub(super) fn prepare_requested_download(&self, hash: &ContentHash) -> SyncResult<Option<TransferJob>> {
        Ok(asset_transfer::planning::prepare_requested_download(&self.assets, &self.database()?, hash)?)
    }
}
