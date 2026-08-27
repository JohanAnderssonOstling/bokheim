use super::queue::ClaimedTransferJob;
use super::{TransferError, TransferOperation, TransferWorker};
use crate::library::database as db;
use crate::platform::library_events::LibraryEvent;
use crate::{BlobKind, BookPlacement, ContentHash, DownloadProgress, DownloadState, TransferJobKind};
use std::collections::{HashMap, HashSet};
use sync_common::api::assets::{ThumbnailBatchDownloadRequest, ThumbnailBatchDownloadResponse, ThumbnailBatchEntry, ThumbnailBatchUploadRequest};

const MAX_THUMBNAIL_BYTES: usize = sync_common::MAX_THUMBNAIL_BYTES as usize;

impl TransferWorker {
    pub(super) async fn handle_claimed_batch(&self, claimed: &[ClaimedTransferJob]) -> Result<(), TransferError> {
        match claimed.first().map(|item| item.job.batch_kind()) {
            Some(Some(TransferJobKind::UploadThumbnail)) => self.upload_thumbnail_batch(claimed).await,
            Some(Some(TransferJobKind::DownloadThumbnail)) => self.download_thumbnail_batch(claimed).await,
            Some(_) => self.handle_job(claimed[0].job.clone()).await,
            None => Ok(()),
        }
    }

    async fn handle_job(&self, job: super::TransferJob) -> Result<(), TransferError> {
        match job.operation {
            TransferOperation::UploadBlob { content_hash, placement } => {
                let _permit = self.budget.clone().acquire_owned().await?;
                self.upload_blob(&content_hash, placement.as_ref()).await
            }
            TransferOperation::DownloadBlob { content_hash, placement } => {
                let _permit = self.budget.clone().acquire_owned().await?;
                self.download_blob(&content_hash, &placement).await
            }
            TransferOperation::UploadThumbnail { .. } | TransferOperation::DownloadThumbnail { .. } => Err(TransferError::rejected("thumbnail job bypassed batch execution")),
        }
    }

    fn session(&self) -> Result<sync_client::Session, TransferError> {
        self.session.read().ok().and_then(|session| session.clone()).ok_or(TransferError::AuthenticationRequired)
    }

    async fn local_bytes(&self, kind: BlobKind, content_hash: &ContentHash) -> Result<Option<Vec<u8>>, TransferError> {
        let stored = {
            let conn = self.db_pool.get()?;
            db::get_local_blob(&conn, kind, content_hash)?
        };
        if let Some(bytes) = stored {
            if kind != BlobKind::Book || blake3::hash(&bytes).to_hex().as_str() == content_hash.as_str() {
                return Ok(Some(bytes));
            }
            let conn = self.db_pool.get()?;
            db::remove_local_blob(&conn, kind, content_hash)?;
        }

        Ok(None)
    }

    async fn upload_thumbnail_batch(&self, claimed: &[ClaimedTransferJob]) -> Result<(), TransferError> {
        let session = self.session()?;
        let mut thumbnails = Vec::with_capacity(claimed.len());
        let mut uploaded = Vec::with_capacity(claimed.len());
        for item in claimed {
            let TransferOperation::UploadThumbnail { thumbnail_hash, .. } = &item.job.operation else { return Err(TransferError::rejected("mixed thumbnail upload batch")) };
            let Some(bytes) = self.local_bytes(BlobKind::Thumbnail, thumbnail_hash).await? else { continue };
            if bytes.len() > MAX_THUMBNAIL_BYTES || !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
                self.discard_invalid_thumbnail(thumbnail_hash).await?;
                return Err(TransferError::rejected("thumbnail is not a bounded JPEG"));
            }
            thumbnails.push(ThumbnailBatchEntry { content_hash: *thumbnail_hash, bytes });
            uploaded.push(*thumbnail_hash);
        }
        if thumbnails.is_empty() {
            return Ok(());
        }
        let body = sync_common::transport::encode(&ThumbnailBatchUploadRequest { thumbnails })?;
        sync_client::upload_thumbnail_batch_body(&self.http_client, session.server_url(), session.token(), &self.library_id, body).await?;
        let conn = self.db_pool.get()?;
        for hash in uploaded {
            db::record_remote_asset(&conn, BlobKind::Thumbnail, &hash)?;
        }
        Ok(())
    }

    async fn download_thumbnail_batch(&self, claimed: &[ClaimedTransferJob]) -> Result<(), TransferError> {
        let session = self.session()?;
        let mut requested = HashMap::<ContentHash, ContentHash>::new();
        for item in claimed {
            let TransferOperation::DownloadThumbnail { thumbnail_hash, book_content_hash } = &item.job.operation else { return Err(TransferError::rejected("mixed thumbnail download batch")) };
            let is_local = {
                let conn = self.db_pool.get()?;
                db::local_blob_exists(&conn, BlobKind::Thumbnail, thumbnail_hash)?
            };
            if is_local {
                self.send_thumbnail_ready(*book_content_hash);
            } else {
                requested.insert(*thumbnail_hash, *book_content_hash);
            }
        }
        if requested.is_empty() {
            return Ok(());
        }
        let body = sync_common::transport::encode(&ThumbnailBatchDownloadRequest { content_hashes: requested.keys().copied().collect() })?;
        let bytes = sync_client::download_thumbnail_batch_body(&self.http_client, session.server_url(), session.token(), &self.library_id, body).await?;
        let response: ThumbnailBatchDownloadResponse = sync_common::transport::decode(&bytes, sync_common::api::assets::MAX_THUMBNAIL_BATCH_BYTES + 1024 * 1024)?;
        let mut answered = HashSet::new();
        for thumbnail in response.thumbnails {
            let Some(book_hash) = requested.get(&thumbnail.content_hash) else { return Err(TransferError::retryable("thumbnail batch returned an unrequested hash")) };
            if !answered.insert(thumbnail.content_hash) || thumbnail.bytes.len() > MAX_THUMBNAIL_BYTES || !thumbnail.bytes.starts_with(&[0xff, 0xd8, 0xff]) {
                return Err(TransferError::retryable("thumbnail batch returned invalid bytes"));
            }
            let conn = self.db_pool.get()?;
            db::put_local_blob(&conn, BlobKind::Thumbnail, &thumbnail.content_hash, &thumbnail.bytes)?;
            db::record_remote_asset(&conn, BlobKind::Thumbnail, &thumbnail.content_hash)?;
            drop(conn);
            self.send_thumbnail_ready(*book_hash);
        }
        let conn = self.db_pool.get()?;
        for hash in response.missing {
            if !requested.contains_key(&hash) || !answered.insert(hash) {
                return Err(TransferError::retryable("thumbnail batch returned an invalid missing hash"));
            }
            db::forget_remote_asset(&conn, BlobKind::Thumbnail, &hash)?;
        }
        if answered.len() != requested.len() {
            return Err(TransferError::retryable("thumbnail batch omitted requested hashes"));
        }
        Ok(())
    }

    fn send_thumbnail_ready(&self, content_hash: ContentHash) {
        if let Some(tx) = &self.event_tx {
            let _ = tx.try_send(LibraryEvent::ThumbnailGenerated { content_hash });
        }
    }

    async fn discard_invalid_thumbnail(&self, content_hash: &ContentHash) -> Result<(), TransferError> {
        let conn = self.db_pool.get()?;
        db::remove_local_blob(&conn, BlobKind::Thumbnail, content_hash)?;
        Ok(())
    }

    pub(super) async fn upload_blob(&self, content_hash: &ContentHash, placement: Option<&BookPlacement>) -> Result<(), TransferError> {
        let session = self.session()?;
        let Some(bytes) = self.local_bytes(BlobKind::Book, content_hash).await? else {
            return Err(TransferError::retryable(format!("local publication bytes are unavailable: {content_hash}")));
        };
        if blake3::hash(&bytes).to_hex().as_str() != content_hash.as_str() {
            return Err(TransferError::rejected(format!("upload hash mismatch for {content_hash}")));
        }
        sync_client::upload_blob_stream(&self.http_client, session.server_url(), session.token(), &self.library_id, content_hash, bytes.len() as u64, reqwest::Body::from(bytes)).await?;
        let conn = self.db_pool.get()?;
        db::record_remote_asset(&conn, BlobKind::Book, content_hash)?;
        if let Some(placement) = placement {
            db::set_book_downloaded(&conn, &placement.rel_path, placement.content_hash, true)?;
        }
        Ok(())
    }

    pub(super) async fn download_blob(&self, content_hash: &ContentHash, placement: &BookPlacement) -> Result<(), TransferError> {
        let session = self.session()?;
        self.send_download_status(*content_hash, DownloadState::Downloading(DownloadProgress::ZERO));
        let response = match sync_client::download_blob_stream(&self.http_client, session.server_url(), session.token(), &self.library_id, content_hash, None).await? {
            sync_client::BlobDownloadResponse::Missing => {
                let conn = self.db_pool.get()?;
                db::forget_remote_asset(&conn, BlobKind::Book, content_hash)?;
                db::complete_asset_request(&conn, BlobKind::Book, content_hash)?;
                self.send_download_status(*content_hash, DownloadState::NotDownloaded);
                return Ok(());
            }
            sync_client::BlobDownloadResponse::RestartRequired => return Err(TransferError::retryable("unexpected full-download restart response")),
            sync_client::BlobDownloadResponse::Content(response) => response,
        };
        let bytes = response.bytes().await.map_err(TransferError::retryable)?.to_vec();
        let actual = blake3::hash(&bytes).to_hex().to_string();
        if actual != content_hash.as_str() {
            let conn = self.db_pool.get()?;
            db::forget_remote_asset(&conn, BlobKind::Book, content_hash)?;
            db::complete_asset_request(&conn, BlobKind::Book, content_hash)?;
            self.send_download_status(*content_hash, DownloadState::NotDownloaded);
            return Err(TransferError::rejected(format!("download hash mismatch expected={content_hash} got={actual}")));
        }
        let conn = self.db_pool.get()?;
        if !db::book_placement_expects_hash(&conn, placement, content_hash)? {
            db::complete_asset_request(&conn, BlobKind::Book, content_hash)?;
            self.send_download_status(*content_hash, DownloadState::NotDownloaded);
            return Ok(());
        }
        drop(conn);

        {
            let conn = self.db_pool.get()?;
            db::put_local_blob(&conn, BlobKind::Book, content_hash, &bytes)?;
        }
        let mut conn = self.db_pool.get()?;
        let transaction = conn.transaction()?;
        db::record_remote_asset(&transaction, BlobKind::Book, content_hash)?;
        db::set_book_downloaded(&transaction, &placement.rel_path, *content_hash, true)?;
        db::complete_asset_request(&transaction, BlobKind::Book, content_hash)?;
        transaction.commit()?;

        self.send_download_status(*content_hash, DownloadState::Downloaded);
        Ok(())
    }

    fn send_download_status(&self, content_hash: ContentHash, state: DownloadState) {
        if let Some(tx) = &self.event_tx {
            let refresh_folders = matches!(state, DownloadState::Downloaded | DownloadState::NotDownloaded);
            let _ = tx.try_send(LibraryEvent::DownloadStatusChanged { content_hash, state });
            if refresh_folders {
                let folder_statuses = (|| -> Result<Vec<_>, Box<dyn std::error::Error>> {
                    let conn = self.db_pool.get()?;
                    db::get_book_ancestor_dir_ids(&conn, content_hash)?
                        .into_iter()
                        .map(|dir_id| db::get_folder_download_counts(&conn, &dir_id.to_string()).map(|(downloaded, total)| (dir_id, downloaded, total)).map_err(Into::into))
                        .collect()
                })();
                if let Ok(statuses) = folder_statuses {
                    for (dir_id, downloaded_books, total_books) in statuses {
                        let _ = tx.try_send(LibraryEvent::FolderDownloadStatusChanged { dir_id, downloaded_books, total_books });
                    }
                }
            }
        }
    }
}
