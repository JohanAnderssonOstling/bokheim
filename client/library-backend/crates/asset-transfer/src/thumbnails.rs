//! Thumbnail transfer execution with host-supplied resizing and notifications.
use crate::deadline::transfer_deadline;
use crate::policy::SMALL_REQUEST_TIMEOUT;
use crate::TransferError;
use crate::transfer::AssetExecutor;
use cpu_host::resize;
use library_files::AssetStore;
use library_model::BlobKind;
use std::collections::HashSet;
use sync_common::BookId;
const MAX_THUMBNAIL_BYTES: usize = sync_common::MAX_THUMBNAIL_BYTES as usize;
use sync_common::api::assets::ThumbnailBatchEntry;

impl AssetExecutor {
    async fn upload_pair(&self, hash: BookId) -> Result<Option<ThumbnailBatchEntry>, TransferError> {
        let assets = self.state.read().await.assets().clone();
        // Writers hold book leases while replacing local variants. An exclusive
        // lease keeps this upload snapshot from mixing two cover generations.
        let _lease = assets.try_exclusive_lease(&hash)?.ok_or_else(|| TransferError::retryable("thumbnail is being updated"))?;
        let Some(bytes) = assets.read_bytes(BlobKind::Thumbnail, &hash).await? else { return Ok(None) };
        if bytes.len() > MAX_THUMBNAIL_BYTES || !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            self.discard_invalid_thumbnail(&assets, hash).await?;
            return Ok(None);
        }
        let browse_bytes = match assets.read_named(library_files::asset_store::thumbnail_variant_name(&hash, thumbnail::BROWSE_THUMBNAIL_WIDTH)).await? {
            Some(bytes) => bytes,
            None => resize(&*self.cpu, bytes.clone(), thumbnail::BROWSE_THUMBNAIL_WIDTH).await.map_err(TransferError::retryable)?,
        };
        if browse_bytes.len() > MAX_THUMBNAIL_BYTES || !browse_bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            self.discard_invalid_thumbnail(&assets, hash).await?;
            return Ok(None);
        }
        Ok(Some(ThumbnailBatchEntry { book_id: hash, bytes, browse_bytes }))
    }

    /// Upload a bounded page in one request, enforcing its aggregate byte bound.
    pub(crate) async fn upload_thumbnail_batch(&self, hashes: &[BookId]) -> Result<(), TransferError> {
        let mut thumbnails = Vec::with_capacity(hashes.len());
        let mut total_bytes = 0usize;
        for hash in hashes {
            let Some(pair) = self.upload_pair(*hash).await? else { continue };
            total_bytes = total_bytes.checked_add(pair.bytes.len() + pair.browse_bytes.len()).ok_or_else(|| TransferError::rejected("thumbnail batch exceeds byte limit"))?;
            if total_bytes > sync_common::api::assets::MAX_THUMBNAIL_BATCH_BYTES {
                return Err(TransferError::rejected("thumbnail batch exceeds byte limit"));
            }
            thumbnails.push(pair);
        }
        if thumbnails.is_empty() {
            return Ok(());
        }
        let credentials = self.credentials().await?;
        let uploaded = thumbnails.iter().map(|thumbnail| thumbnail.book_id).collect::<Vec<_>>();
        let response = transfer_deadline(SMALL_REQUEST_TIMEOUT, crate::upload_thumbnail_batch(&self.http_client, credentials.server_url(), credentials.access_token(), &self.library_id, thumbnails)).await?;
        self.check_transfer_account(&credentials).await?;
        response?;
        self.database.record_server_covers(&uploaded).await?;
        Ok(())
    }
    /// Covers that are ready, received or missing are written once for the
    /// batch, also when it fails partway: what was stored still counts.
    pub(crate) async fn download_thumbnail_batch(&self, hashes: &[BookId]) -> Result<(), TransferError> {
        let assets = self.state.read().await.assets().clone();
        let mut ready = Vec::new();
        let mut received = Vec::new();
        let mut missing = Vec::new();
        let result: Result<(), TransferError> = async {
            let mut remaining = HashSet::<BookId>::new();
            for thumbnail_hash in hashes {
                let is_local = assets.thumbnail_set_exists(thumbnail_hash).await?;
                if is_local {
                    ready.push(*thumbnail_hash);
                    continue;
                }
                // Fetch missing variants from the server instead of rebuilding
                // them from a previously downloaded master on this device.
                remaining.insert(*thumbnail_hash);
            }
            if remaining.is_empty() {
                return Ok(());
            }
            let credentials = self.credentials().await?;
            let response = transfer_deadline(SMALL_REQUEST_TIMEOUT, crate::download_thumbnail_batch(&self.http_client, credentials.server_url(), credentials.access_token(), &self.library_id, remaining.iter().copied().collect())).await?;
            self.check_transfer_account(&credentials).await?;
            let response = response?;
            for thumbnail in response.thumbnails {
                if !remaining.remove(&thumbnail.book_id) {
                    return Err(TransferError::retryable("thumbnail batch returned an unexpected or duplicate hash"));
                }
                if thumbnail.bytes.len() > MAX_THUMBNAIL_BYTES || !thumbnail.bytes.starts_with(&[0xff, 0xd8, 0xff]) {
                    return Err(TransferError::retryable("thumbnail batch returned invalid bytes"));
                }
                if thumbnail.browse_bytes.len() > MAX_THUMBNAIL_BYTES || !thumbnail.browse_bytes.starts_with(&[0xff, 0xd8, 0xff]) {
                    return Err(TransferError::retryable("thumbnail batch returned invalid browse bytes"));
                }
                // Server artwork is already finished. The transfer supplies a
                // browse JPEG and a master; use the master for dense cards too,
                // preserving both images without cropping or re-encoding.
                assets.write_thumbnail_set(&thumbnail.book_id, &thumbnail.bytes, &thumbnail.browse_bytes, &thumbnail.bytes).await?;
                self.check_transfer_account(&credentials).await?;
                received.push(thumbnail.book_id);
                ready.push(thumbnail.book_id);
            }
            self.check_transfer_account(&credentials).await?;
            for hash in response.missing {
                if !remaining.remove(&hash) {
                    return Err(TransferError::retryable("thumbnail batch returned an invalid missing hash"));
                }
                missing.push(hash);
            }
            if !remaining.is_empty() {
                return Err(TransferError::retryable("thumbnail batch omitted requested hashes"));
            }
            Ok(())
        }
        .await;
        self.database.complete_cover_downloads(&ready, &received, &missing).await?;
        for hash in ready {
            self.events.cover_changed(hash);
        }
        result
    }
    /// The caller retains its exclusive book lease through deletion and retry scheduling.
    async fn discard_invalid_thumbnail(&self, assets: &AssetStore, book_id: BookId) -> Result<(), TransferError> {
        assets.remove(BlobKind::Thumbnail, &book_id).await?;
        self.database.request_missing_thumbnail(&book_id).await?;
        Ok(())
    }
}
