//! Verified book upload/download workflows with host-supplied resources.
pub(crate) use super::deadline::transfer_deadline;
pub(crate) use crate::policy::upload_timeout;
pub(crate) use crate::TransferError;
use crate::transfer::AssetExecutor;
#[cfg(not(target_arch = "wasm32"))]
use client_platform_runtime::executor::run_blocking;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use futures_util::StreamExt;
use library_database::TransferSnapshot;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use library_database::NativeBookCompletion;
pub(crate) use library_model::BookUploadIntent;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use library_model::{BookPlacement, DownloadProgress, DownloadState};
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use sync_common::BookId;

#[cfg(target_arch = "wasm32")]
pub mod web_upload;

impl AssetExecutor {
    /// Upload a version already admitted by the page's manifest negotiation.
    pub(crate) async fn upload_blob_version(&self, snapshot: &TransferSnapshot, planned: &BookUploadIntent) -> Result<(), TransferError> {
        let assets = self.state.read().await.assets().clone();
        let book_id = &planned.book_id;
        let credentials = self.credentials().await?;
        let asset = snapshot.asset_for(book_id).ok_or_else(|| TransferError::retryable("transfer snapshot changed during drain"))?;
        let Some(intent) = asset.upload.intent.as_ref() else {
            return Ok(());
        };
        if planned != intent {
            return Err(TransferError::retryable("upload intent changed during transfer"));
        }
        #[cfg(not(target_arch = "wasm32"))]
        let format = self.database.local_book_info(book_id).await?.format;
        #[cfg(target_arch = "wasm32")]
        let canonical_pdf = self.database.pdf_metadata(book_id).await?;
        let file_name = asset.upload.local_versions.first().map(|(_, file_name, _)| file_name.as_str()).unwrap_or("unknown file");
        let Some(book) = assets.verified_book_reader_with_checksum(*book_id, asset.upload_paths.clone(), intent.byte_checksum).await? else {
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
        let (book, canonical_pdf) = if format == book_model::BookFormat::Pdf {
            let identity = book_id.to_hex();
            let (book, pdf, _) = client_platform_runtime::executor::run_blocking(move || pdf_reader_core::inspect_reader_metadata_with_identity(book, identity)).await.map_err(TransferError::retryable)?.map_err(TransferError::retryable)?;
            (book, Some(pdf))
        } else {
            (book, None)
        };
        let uploaded_size = book.length;
        let uploaded_checksum = book.book_id.content_hash().ok_or_else(|| TransferError::rejected("verified book checksum is unavailable"))?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            transfer_deadline(
                upload_timeout(intent.size_bytes),
                sync_transport::upload_book_stream_with_checksum(
                    &self.http_client,
                    credentials.server_url(),
                    credentials.access_token(),
                    &self.library_id,
                    book_id,
                    Some(uploaded_checksum),
                    intent.size_bytes,
                    reqwest::Body::wrap_stream(book.into_upload_stream()),
                ),
            )
            .await?
            .map_err(TransferError::from)?;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let endpoint = sync_transport::blob_endpoint(credentials.server_url(), &self.library_id, book_id).map_err(crate::AssetResponseError::Url)?;
            transfer_deadline(upload_timeout(book.length), web_upload::upload(&book, &endpoint, credentials.access_token())).await??;
        }
        #[cfg(target_arch = "wasm32")]
        drop(book);
        self.database.confirm_uploaded_content_with_pdf(*book_id, uploaded_checksum, Some(uploaded_size), canonical_pdf).await?;
        Ok(())
    }

    /// Native only: the browser streams books it does not hold instead of
    /// downloading them.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) async fn download_blob(&self, snapshot: &TransferSnapshot, book_id: &BookId, placement: &BookPlacement) -> Result<(), TransferError> {
        let assets = self.state.read().await.assets().clone();
        // No live placement expects this download: the placement went stale
        // (or never named this hash), so there is nothing to publish. This
        // mirrors the currency skip below; the queue job still completes.
        let Some(entry) = snapshot.placement_for(book_id, &placement.rel_path) else {
            return Ok(());
        };
        let Some(book) = entry.book.clone() else {
            return Ok(());
        };
        let credentials = self.credentials().await?;
        self.send_download_status(*book_id, DownloadState::Downloading(DownloadProgress::ZERO));
        let endpoint = sync_transport::blob_endpoint(credentials.server_url(), &self.library_id, book_id).map_err(crate::AssetResponseError::Url)?;
        let response = self.http_client.get(endpoint).bearer_auth(credentials.access_token()).send().await.map_err(crate::AssetResponseError::Retryable)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            self.database.cancel_download(book_id).await?;
            return Ok(());
        }
        let response = crate::validate_streaming_asset_response(response)?;
        let server_checksum = response
            .headers()
            .get("x-bokheim-content-hash")
            .map(|value| value.to_str().ok().and_then(|value| value.parse::<content_address::ContentHash>().ok()).ok_or_else(|| TransferError::rejected("invalid server book checksum")))
            .transpose()?;
        let known_checksum = self.database.canonical_content_hash(*book_id).await?;
        if known_checksum.zip(server_checksum).is_some_and(|(known, server)| known != server) {
            return Err(TransferError::rejected("server book revision differs from its canonical checksum"));
        }
        let checksum = known_checksum.or(server_checksum).or_else(|| book_id.content_hash()).ok_or_else(|| TransferError::rejected("book checksum is unavailable"))?;
        let total_bytes = response.content_length();
        let mut destination = {
            assets.ensure_placement_parent(&placement.rel_path)?;
            assets.begin_book_write(&placement.rel_path, book_id)?
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
                self.database.cancel_download(book_id).await?;
                return Err(TransferError::rejected(error.to_string()));
            }
        };
        // A local flush/sync failure says nothing about the server's copy.
        // Background work retries it; explicit requests are reported by the executor.
        let destination = destination.finish_book().await?;
        let assets = assets.clone();
        let planned = book.clone();
        let root_access = assets.lock_root_shared().await;
        let lease = assets.lease_book(book_id).await?;
        let (prepared, root_access) = run_blocking(move || {
            // Directory moves and eviction must not relocate an Android
            // compatibility copy while it is still being written. Release
            // this lease before waiting for the library gate below.
            let _lease = lease;
            (destination.prepare_download(&assets, &planned.relative_path, &planned.occupied_names, planned.format == book_model::BookFormat::Mp3Folder, &planned.audiobook_track_titles), root_access)
        })
        .await
        .map_err(TransferError::retryable)?;
        let mut prepared = prepared?;
        let result = async {
            let storage = self.state.write().await;
            // Revalidate the planned placement before publishing downloaded bytes.
            let current = storage.database().transfer_snapshot(&[*book_id]).await?;
            let Some(current) = current.placement_for(book_id, &placement.rel_path).and_then(|entry| entry.book.as_ref()).filter(|current| {
                current.directory == book.directory && current.name == book.name && current.relative_path == book.relative_path && current.format == book.format && current.audiobook_track_titles == book.audiobook_track_titles
            }) else {
                return Ok(());
            };
            let published = prepared.publish(storage.assets(), &current.occupied_names)?;
            let completion = NativeBookCompletion { snapshot: current.clone(), publication: published, size_bytes: completed_bytes };
            storage.database().complete_downloaded_book_with_content(completion, checksum, storage.assets().thumbnail_set_exists(book_id).await?).await?;
            Ok(())
        }
        .await;
        // Stale/duplicate downloads may own an entire extracted tree. Cleanup
        // must also run off the library thread, after releasing the gate.
        run_blocking(move || {
            let _root = root_access;
            drop(prepared);
        })
        .await
        .map_err(TransferError::retryable)?;
        result
    }
}
