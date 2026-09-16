use super::{TransferError, TransferOperation, TransferWorker};
use asset_transfer::ClaimedTransferJob;
use client_library::BookPlacement;
use library_model::DownloadState;
use library_replica::TransferJobKind;
use library_runtime::database::Database;
use library_runtime::events::LibraryEvent;
use sync_common::ContentHash;

impl TransferWorker {
    pub(super) async fn prepare_book_uploads_with_refresh(&self, hashes: &[ContentHash]) -> Result<Vec<super::TransferJob>, TransferError> {
        if !self.asset_storage_enabled() {
            return Ok(Vec::new());
        }
        self.with_refresh(super::RefreshScope::Planned, || self.prepare_book_uploads(hashes)).await
    }

    /// Negotiate one bounded planner page before opening or hashing any files.
    /// Only missing blobs become transfer jobs; existing versions complete here.
    pub(in crate::sync) async fn prepare_book_uploads(&self, hashes: &[ContentHash]) -> Result<Vec<super::TransferJob>, TransferError> {
        if !self.asset_storage_enabled() {
            return Ok(Vec::new());
        }
        self.transfer_planner().prepare_book_uploads(hashes).await
    }

    pub(in crate::sync) async fn prepare_thumbnail_uploads_with_refresh(&self, hashes: &[ContentHash]) -> Result<Vec<super::TransferJob>, TransferError> {
        if !self.asset_storage_enabled() {
            return Ok(Vec::new());
        }
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        self.with_refresh(super::RefreshScope::Planned, || self.prepare_thumbnail_uploads(hashes)).await
    }

    /// Check remote availability before publishing upload activity. The existing
    /// bounded thumbnail endpoint also works with older servers.
    pub(in crate::sync) async fn prepare_thumbnail_uploads(&self, hashes: &[ContentHash]) -> Result<Vec<super::TransferJob>, TransferError> {
        if !self.asset_storage_enabled() {
            return Ok(Vec::new());
        }
        self.transfer_planner().prepare_thumbnail_uploads(hashes).await
    }

    pub(super) fn resolve_rejected_job(&self, job: &super::TransferJob, reason: &str) -> Result<(), TransferError> {
        use client_library::assets::RejectedAsset;
        let asset = match &job.operation {
            TransferOperation::DownloadBlob { content_hash, .. } => RejectedAsset::BookDownload(content_hash),
            TransferOperation::UploadBlob { content_hash, intent, .. } => RejectedAsset::BookUpload { hash: content_hash, intent: intent.as_ref() },
            TransferOperation::UploadThumbnail { thumbnail_hash } => RejectedAsset::ThumbnailUpload(thumbnail_hash),
            TransferOperation::DownloadThumbnail { .. } => return Ok(()),
        };
        client_library::assets::settle_rejected_asset(&mut *self.db_pool.get()?, asset, reason)?;
        if let TransferOperation::DownloadBlob { content_hash, .. } = &job.operation {
            self.send_download_status(*content_hash, DownloadState::NotDownloaded);
        }
        Ok(())
    }

    pub(super) async fn handle_claimed_batch(&self, claimed: &[ClaimedTransferJob]) -> Result<(), TransferError> {
        match claimed.first().map(|item| item.job.batch_kind()) {
            Some(Some(TransferJobKind::DownloadThumbnail)) => self.download_thumbnail_batch(claimed).await,
            Some(_) => self.handle_job(claimed[0].job.clone()).await,
            None => Ok(()),
        }
    }

    pub(super) async fn execute_job(&self, job: super::TransferJob) -> Result<(), TransferError> {
        match job.operation {
            TransferOperation::UploadBlob { content_hash, intent, negotiated } => self.upload_blob_version(&content_hash, intent.as_ref(), negotiated).await,
            TransferOperation::DownloadBlob { content_hash, placement } => {
                if self.assets.complete_retained_download(&*self.db_pool.get()?, &content_hash, &placement)? {
                    return Ok(());
                }
                self.download_blob(&content_hash, &placement).await
            }
            TransferOperation::UploadThumbnail { thumbnail_hash, .. } => self.upload_thumbnail(&thumbnail_hash).await,
            TransferOperation::DownloadThumbnail { .. } => Err(TransferError::rejected("thumbnail download job bypassed batch execution")),
        }
    }

    pub(super) fn credentials(&self) -> Result<sync_transport::SyncCredentials, TransferError> {
        self.credentials.credentials().ok_or(TransferError::AuthenticationRequired)
    }

    pub(super) fn check_transfer_account(&self, conn: &rusqlite::Connection, user: &Option<String>, credentials: &sync_transport::SyncCredentials) -> Result<(), TransferError> {
        asset_transfer::publications::validate_account(conn, user, credentials, || self.credentials())
    }

    async fn upload_thumbnail(&self, thumbnail_hash: &ContentHash) -> Result<(), TransferError> {
        if !self.asset_storage_enabled() {
            return Ok(());
        }
        self.thumbnail_transfers().upload_thumbnail(thumbnail_hash).await
    }

    async fn download_thumbnail_batch(&self, claimed: &[ClaimedTransferJob]) -> Result<(), TransferError> {
        self.thumbnail_transfers().download_thumbnail_batch(claimed).await
    }

    fn transfer_planner(&self) -> asset_transfer::planning::TransferPlanner<Database, impl Fn() -> Result<sync_transport::SyncCredentials, TransferError> + Sync + '_> {
        asset_transfer::planning::TransferPlanner { db_pool: self.db_pool.clone(), http_client: self.http_client.clone(), library_id: self.library_id, credentials: || self.credentials() }
    }
    fn thumbnail_transfers(
        &self,
    ) -> asset_transfer::thumbnails::ThumbnailTransfers<
        Database,
        impl Fn() -> Result<sync_transport::SyncCredentials, TransferError> + Sync + '_,
        impl Fn(Vec<u8>, u32) -> client_platform_runtime::executor::BoxedBackendFuture<'static, Result<Vec<u8>, String>> + Sync,
        impl Fn(ContentHash) + Sync + '_,
    > {
        asset_transfer::thumbnails::ThumbnailTransfers {
            db_pool: self.db_pool.clone(),
            assets: self.assets.clone(),
            http_client: self.http_client.clone(),
            library_id: self.library_id,
            credentials: || self.credentials(),
            resize: |bytes, width| -> client_platform_runtime::executor::BoxedBackendFuture<'static, Result<Vec<u8>, String>> { Box::pin(crate::worker_cpu::resize(bytes, width)) },
            ready: |content_hash| {
                if let Some(tx) = &self.event_tx {
                    let _ = tx.try_send(LibraryEvent::ThumbnailGenerated { content_hash });
                }
            },
        }
    }

    pub(super) fn publication_transfers(
        &self,
    ) -> asset_transfer::publications::PublicationTransfers<Database, impl Fn() -> Result<sync_transport::SyncCredentials, TransferError> + Sync + '_, impl Fn(ContentHash, DownloadState) + Sync + '_> {
        asset_transfer::publications::PublicationTransfers {
            db_pool: self.db_pool.clone(),
            assets: self.assets.clone(),
            http_client: self.http_client.clone(),
            library_id: self.library_id,
            credentials: || self.credentials(),
            status: |hash, state| self.send_download_status(hash, state),
            diagnostic: |_| {},
        }
    }

    pub(super) async fn upload_blob(&self, content_hash: &ContentHash) -> Result<(), TransferError> {
        if !self.asset_storage_enabled() {
            return Ok(());
        }
        self.publication_transfers().upload_blob(content_hash).await
    }

    async fn upload_blob_version(&self, content_hash: &ContentHash, planned: Option<&client_library::assets::BookUploadIntent>, negotiated: bool) -> Result<(), TransferError> {
        if !self.asset_storage_enabled() {
            return Ok(());
        }
        self.publication_transfers().upload_blob_version(content_hash, planned, negotiated).await
    }

    pub(super) async fn download_blob(&self, content_hash: &ContentHash, placement: &BookPlacement) -> Result<(), TransferError> {
        self.publication_transfers().download_blob(content_hash, placement).await
    }

    /// Reports one book's availability to whoever is showing that book.
    ///
    /// Downloaded and cleared books also change the counts folders display, but
    /// that is a property of the batch rather than of each book in it: a run
    /// that fetches four hundred books changes those counts once. The batch
    /// boundary announces it — see [`TransferWorker::flush_availability_changes`].
    fn send_download_status(&self, content_hash: ContentHash, state: DownloadState) {
        if matches!(state, DownloadState::Downloaded | DownloadState::NotDownloaded) {
            self.availability_changed.store(true, std::sync::atomic::Ordering::Release);
        }
        if let Some(tx) = &self.event_tx {
            let _ = tx.try_send(LibraryEvent::DownloadStatusChanged { content_hash, state });
        }
    }
}
