//! Verified book upload/download workflows with host-supplied resources.
pub(crate) use super::deadline::transfer_deadline;
pub(crate) use crate::policy::upload_timeout;
pub(crate) use crate::TransferError;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use futures_util::StreamExt;
use library_database::Database;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use library_database::NativeBookCompletion;
pub(crate) use library_files::AssetStore;
pub(crate) use library_model::BookUploadIntent;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use library_model::{BookPlacement, DownloadProgress, DownloadState};
use library_runtime::account::SharedSyncCredentials;
#[cfg(not(target_arch = "wasm32"))]
use library_runtime::events::LibraryEvent;
use library_runtime::events::LibraryEventRouter;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::Ordering;
use std::sync::{atomic::AtomicBool, Arc};
pub(crate) use sync_common::BookId;
use sync_common::LibraryId;
use sync_transport::SyncCredentials;

#[cfg(target_arch = "wasm32")]
pub mod web_upload;

/// Credentials and notifications are supplied by the host; execution owns the
/// byte lifecycle, revision checks, and the guarded completion writes.
pub struct BookTransfers {
    pub assets: AssetStore,
    pub database: Arc<Database>,
    pub http_client: reqwest::Client,
    pub library_id: LibraryId,
    pub credentials: SharedSyncCredentials,
    pub availability_changed: Arc<AtomicBool>,
    pub events: Option<LibraryEventRouter>,
}
impl BookTransfers {
    pub(crate) fn credentials(&self) -> Result<SyncCredentials, TransferError> {
        self.credentials.credentials().ok_or(TransferError::AuthenticationRequired)
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn send_download_status(&self, hash: BookId, state: DownloadState) {
        report_download_status(&self.availability_changed, self.events.as_ref(), hash, state);
    }

    /// Upload a version already admitted by the page's manifest negotiation.
    pub async fn upload_blob_version(&self, snapshot: &library_database::TransferSnapshot, planned: &BookUploadIntent) -> Result<(), TransferError> {
        let book_id = &planned.book_id;
        let credentials = self.credentials()?;
        let asset = snapshot.asset_for(book_id).ok_or_else(|| TransferError::retryable("transfer snapshot changed during drain"))?;
        let Some(intent) = asset.upload.intent.as_ref() else {
            return Ok(());
        };
        if planned != intent {
            return Err(TransferError::retryable("upload intent changed during transfer"));
        }
        let file_name = asset.upload.local_versions.first().map(|(_, file_name, _)| file_name.as_str()).unwrap_or("unknown file");
        let Some(book) = self.assets.verified_book_reader_with_checksum(*book_id, asset.upload_paths.clone(), intent.byte_checksum).await? else {
            // Verification rejects missing or mismatching bytes without deleting
            // other local revisions. Retrying that same job forever would
            // pin the queue behind one pathological file.
            return Err(TransferError::rejected(format!("local book is unavailable or invalid: {file_name}")));
        };
        if book.length == 0 {
            return Err(TransferError::rejected(format!("local book is empty: {file_name}")));
        }
        if intent.size_bytes != book.length {
            return Err(TransferError::retryable("local bytes do not match queued book version"));
        }
        #[cfg(not(target_arch = "wasm32"))]
        let (book, canonical_pdf) = if self.database.local_book_info(book_id)?.format == book_model::BookFormat::Pdf {
            let identity = book_id.to_hex();
            let (book, pdf, _) = client_platform_runtime::executor::run_blocking(move || pdf_reader_core::inspect_reader_metadata_with_identity(book, identity))
                .await.map_err(TransferError::retryable)?.map_err(TransferError::retryable)?;
            (book, Some(pdf))
        } else { (book, None) };
        #[cfg(target_arch = "wasm32")]
        let canonical_pdf = self.database.pdf_metadata(book_id)?;
        let uploaded_size = book.length;
        let uploaded_checksum = book.book_id.content_hash().ok_or_else(|| TransferError::rejected("verified book checksum is unavailable"))?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            transfer_deadline(
                upload_timeout(intent.size_bytes),
                sync_transport::upload_book_stream_with_checksum(&self.http_client, credentials.server_url(), credentials.access_token(), &self.library_id, book_id, intent.byte_checksum, intent.size_bytes, reqwest::Body::wrap_stream(book.into_upload_stream())),
            )
            .await?
            .map_err(TransferError::from)?;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let endpoint = sync_transport::blob_endpoint(credentials.server_url(), &self.library_id, book_id).map_err(crate::AssetResponseError::Url)?;
            transfer_deadline(upload_timeout(book.length), web_upload::upload(&book, &endpoint, credentials.access_token())).await??;
        }
        self.database.confirm_uploaded_content_with_pdf(*book_id, uploaded_checksum, Some(uploaded_size), canonical_pdf)?;
        Ok(())
    }

    /// Native only: the browser streams books it does not hold instead of
    /// downloading them.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn download_blob(&self, snapshot: &library_database::TransferSnapshot, book_id: &BookId, placement: &BookPlacement) -> Result<(), TransferError> {
        // No live placement expects this download: the placement went stale
        // (or never named this hash), so there is nothing to publish. This
        // mirrors the currency skip below; the queue job still completes.
        let Some(entry) = snapshot.placement_for(book_id, &placement.rel_path) else {
            return Ok(());
        };
        let Some(book) = entry.book.clone() else {
            return Ok(());
        };
        let credentials = self.credentials()?;
        self.send_download_status(*book_id, DownloadState::Downloading(DownloadProgress::ZERO));
        let endpoint = sync_transport::blob_endpoint(credentials.server_url(), &self.library_id, book_id).map_err(crate::AssetResponseError::Url)?;
        let response = self.http_client.get(endpoint).bearer_auth(credentials.access_token()).send().await.map_err(crate::AssetResponseError::Retryable)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            self.database.cancel_download(book_id)?;
            return Ok(());
        }
        let response = crate::validate_streaming_asset_response(response)?;
        let server_checksum = response.headers().get("x-bokheim-content-hash").map(|value| value.to_str().ok().and_then(|value| value.parse::<content_address::ContentHash>().ok()).ok_or_else(|| TransferError::rejected("invalid server book checksum"))).transpose()?;
        let known_checksum = self.database.canonical_content_hash(*book_id)?;
        if known_checksum.zip(server_checksum).is_some_and(|(known, server)| known != server) {
            return Err(TransferError::rejected("server book revision differs from its canonical checksum"));
        }
        let checksum = known_checksum.or(server_checksum).or_else(|| book_id.content_hash()).ok_or_else(|| TransferError::rejected("book checksum is unavailable"))?;
        let total_bytes = response.content_length();
        self.assets.check_write_capacity(total_bytes.unwrap_or(1))?;
        let mut destination = {
            self.assets.ensure_placement_parent(&placement.rel_path)?;
            self.assets.begin_book_write(&placement.rel_path, book_id).await?
        };
        let mut completed_bytes = 0_u64;
        let mut reported_percent = 0_u8;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(TransferError::retryable)?;
            completed_bytes = completed_bytes.saturating_add(chunk.len() as u64);
            destination.write_all(&chunk).await?;
            if let Some(total) = total_bytes.filter(|total| *total > 0) {
                let progress = DownloadProgress::from_ratio(completed_bytes, total);
                let percent = (progress.value() * 100.0) as u8;
                if percent > reported_percent {
                    reported_percent = percent;
                    self.send_download_status(*book_id, DownloadState::Downloading(progress));
                }
            }
        }
        let destination = match destination.verify_checksum(checksum).await {
            Ok(destination) => destination,
            Err(error) => {
                self.database.cancel_download(book_id)?;
                return Err(TransferError::rejected(error.to_string()));
            }
        };
        // A local flush/sync failure says nothing about the server's copy.
        // Background work retries it; explicit requests are reported by the executor.
        let destination = destination.finish_book().await?;
        let published = destination.publish_snapshot_with_track_titles(&self.assets, &book.relative_path, &book.occupied_names, book.format == book_model::BookFormat::Mp3Folder, &book.audiobook_track_titles)?;
        // The write revalidates the placement and name conflicts before applying the result.
        let completion = NativeBookCompletion { snapshot: book, publication: published, size_bytes: completed_bytes };
        self.database.complete_downloaded_book_with_content(completion, checksum, self.assets.thumbnail_set_exists(book_id)?)?;
        Ok(())
    }
}

/// Shares completion tracking and progress delivery with the asset executor.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn report_download_status(availability_changed: &AtomicBool, events: Option<&LibraryEventRouter>, book_id: BookId, state: DownloadState) {
    if matches!(state, DownloadState::Downloaded | DownloadState::NotDownloaded) {
        availability_changed.store(true, Ordering::Release);
    }
    if let Some(events) = events {
        events.emit(LibraryEvent::DownloadStatusChanged { book_id, state });
    }
}
