//! Thumbnail transfer execution with host-supplied resizing and notifications.
use crate::deadline::transfer_deadline;
use crate::policy::SMALL_REQUEST_TIMEOUT;
use crate::{ClaimedTransferJob, TransferError, TransferOperation};
use library_files::AssetStore;
use library_replica::BlobKind;
use sync_common::ContentHash;
use std::collections::HashSet;
const MAX_THUMBNAIL_BYTES: usize = sync_common::MAX_THUMBNAIL_BYTES as usize;
use sync_common::api::assets::ThumbnailBatchEntry;

/// Owns thumbnail byte handling and persistence; the host supplies CPU work and events.
pub struct ThumbnailTransfers<C, R, N> {
    pub assets: AssetStore,
    pub http_client: reqwest::Client,
    pub library_id: sync_common::LibraryId,
    pub credentials: C,
    pub resize: R,
    pub ready: N,
}
impl<C, R, N> ThumbnailTransfers<C, R, N>
where
    C: Fn() -> Result<sync_transport::SyncCredentials, TransferError> + Sync,
    R: Fn(Vec<u8>, u32) -> client_platform_runtime::executor::BoxedBackendFuture<'static, Result<Vec<u8>, String>> + Sync,
    N: Fn(ContentHash) + Sync,
{
    fn credentials(&self) -> Result<sync_transport::SyncCredentials, TransferError> {
        (self.credentials)()
    }
    fn check_transfer_account(&self, credentials: &sync_transport::SyncCredentials) -> Result<(), TransferError> {
        if self.credentials()?.server_url() != credentials.server_url() {
            return Err(TransferError::retryable("credentials changed during asset transfer"));
        }
        Ok(())
    }
    pub async fn upload_thumbnail(&self, snapshot: &library_database::TransferSnapshot, thumbnail_hash: &ContentHash, out: &mut library_database::TransferOutcome) -> Result<(), TransferError> {
        let credentials = self.credentials()?;
        let Some(bytes) = self.assets.read_bytes(BlobKind::Thumbnail, thumbnail_hash).await? else { return Ok(()) };
        if bytes.len() > MAX_THUMBNAIL_BYTES || !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            self.discard_invalid_thumbnail(*thumbnail_hash, out).await?;
            log::warn!("Discarded invalid local thumbnail {thumbnail_hash}; a regenerated thumbnail may be uploaded immediately");
            return Ok(());
        }
        let _ = snapshot;
        let response = transfer_deadline(
            SMALL_REQUEST_TIMEOUT,
            crate::upload_thumbnail_batch(&self.http_client, credentials.server_url(), credentials.access_token(), &self.library_id, vec![ThumbnailBatchEntry { content_hash: *thumbnail_hash, bytes }]),
        )
        .await?;
        self.check_transfer_account(&credentials)?;
        response?;
        out.record(library_database::TransferWrite::RecordUploadedThumbnail { hash: *thumbnail_hash });
        Ok(())
    }
    pub async fn download_thumbnail_batch(&self, snapshot: &library_database::TransferSnapshot, claimed: &[ClaimedTransferJob], out: &mut library_database::TransferOutcome) -> Result<(), TransferError> {
        let _ = snapshot;
        let mut requested = HashSet::<ContentHash>::new();
        for item in claimed {
            let TransferOperation::DownloadThumbnail { thumbnail_hash } = &item.job.operation else { return Err(TransferError::rejected("mixed thumbnail download batch")) };
            let is_local = self.assets.thumbnail_set_exists(thumbnail_hash)?;
            if is_local {
                self.send_thumbnail_ready(*thumbnail_hash, out)?;
                continue;
            }
            if let Some(bytes) = self.assets.read_bytes(BlobKind::Thumbnail, thumbnail_hash).await? {
                if bytes.len() <= MAX_THUMBNAIL_BYTES && bytes.starts_with(&[0xff, 0xd8, 0xff]) {
                    if let Ok(browse) = (self.resize)(bytes, thumbnail::BROWSE_THUMBNAIL_WIDTH).await {
                        self.assets.write_thumbnail_variant(thumbnail_hash, thumbnail::BROWSE_THUMBNAIL_WIDTH, &browse).await?;
                        self.send_thumbnail_ready(*thumbnail_hash, out)?;
                        continue;
                    }
                }
            }
            requested.insert(*thumbnail_hash);
        }
        if requested.is_empty() {
            return Ok(());
        }
        let credentials = self.credentials()?;
        let response = transfer_deadline(SMALL_REQUEST_TIMEOUT, crate::download_thumbnail_batch(&self.http_client, credentials.server_url(), credentials.access_token(), &self.library_id, requested.iter().copied().collect())).await?;
        self.check_transfer_account(&credentials)?;
        let response = response?;
        let mut answered = HashSet::new();
        for thumbnail in response.thumbnails {
            if !requested.contains(&thumbnail.content_hash) {
                return Err(TransferError::retryable("thumbnail batch returned an unrequested hash"));
            }
            if !answered.insert(thumbnail.content_hash) || thumbnail.bytes.len() > MAX_THUMBNAIL_BYTES || !thumbnail.bytes.starts_with(&[0xff, 0xd8, 0xff]) {
                return Err(TransferError::retryable("thumbnail batch returned invalid bytes"));
            }
            self.assets.write(BlobKind::Thumbnail, &thumbnail.content_hash, &thumbnail.bytes).await?;
            let browse = (self.resize)(thumbnail.bytes, thumbnail::BROWSE_THUMBNAIL_WIDTH).await.map_err(TransferError::retryable)?;
            self.assets.write_thumbnail_variant(&thumbnail.content_hash, thumbnail::BROWSE_THUMBNAIL_WIDTH, &browse).await?;
            self.check_transfer_account(&credentials)?;
            out.record(library_database::TransferWrite::CompleteDownloadedThumbnail { hash: thumbnail.content_hash });
            self.send_thumbnail_ready(thumbnail.content_hash, out)?;
        }
        self.check_transfer_account(&credentials)?;
        for hash in response.missing {
            if !requested.contains(&hash) || !answered.insert(hash) {
                return Err(TransferError::retryable("thumbnail batch returned an invalid missing hash"));
            }
            out.record(library_database::TransferWrite::ForgetRemoteThumbnail { hash });
        }
        if answered.len() != requested.len() {
            return Err(TransferError::retryable("thumbnail batch omitted requested hashes"));
        }
        Ok(())
    }
    fn send_thumbnail_ready(&self, content_hash: ContentHash, out: &mut library_database::TransferOutcome) -> Result<(), TransferError> {
        out.record(library_database::TransferWrite::ConfirmThumbnailAvailable { hash: content_hash });
        (self.ready)(content_hash);
        Ok(())
    }
    async fn discard_invalid_thumbnail(&self, content_hash: ContentHash, out: &mut library_database::TransferOutcome) -> Result<(), TransferError> {
        self.assets.remove(BlobKind::Thumbnail, &content_hash)?;
        out.record(library_database::TransferWrite::RequestMissingThumbnail { hash: content_hash });
        Ok(())
    }
}
