//! Verified book upload/download workflows with host-supplied resources.
pub(crate) use super::deadline::transfer_deadline;
pub(crate) use crate::policy::{upload_timeout, SMALL_REQUEST_TIMEOUT};
pub(crate) use crate::TransferError;
pub(crate) use futures_util::StreamExt;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use library_database::NativeBookCompletion;
pub(crate) use library_files::AssetStore;
#[cfg(target_arch = "wasm32")]
pub(crate) use library_model::BlobKind;
pub(crate) use library_model::BookPlacement;
pub(crate) use library_model::{DownloadProgress, DownloadState};
pub(crate) use sync_common::ContentHash;

#[cfg(target_arch = "wasm32")]
pub mod web_upload;

/// Credentials and notifications are supplied by the host; execution owns the
/// byte lifecycle, revision checks, and conditional completion records.
pub struct BookTransfers<C, S> {
    pub assets: AssetStore,
    pub http_client: reqwest::Client,
    pub library_id: sync_common::LibraryId,
    pub credentials: C,
    pub status: S,
}
impl<C, S> BookTransfers<C, S>
where
    C: Fn() -> Result<sync_transport::SyncCredentials, TransferError> + Sync,
    S: Fn(ContentHash, DownloadState) + Sync,
{
    pub(crate) fn credentials(&self) -> Result<sync_transport::SyncCredentials, TransferError> {
        (self.credentials)()
    }
    pub(crate) fn send_download_status(&self, hash: ContentHash, state: DownloadState) {
        (self.status)(hash, state);
    }

    pub async fn upload_blob_version(
        &self, snapshot: &library_database::TransferSnapshot, content_hash: &ContentHash, planned: Option<&library_model::BookUploadIntent>, already_admitted: bool, out: &mut library_database::TransferOutcome,
    ) -> Result<(), TransferError> {
        let credentials = self.credentials()?;
        let asset = snapshot.asset_for(content_hash).ok_or_else(|| TransferError::retryable("transfer snapshot changed during drain"))?;
        let Some(intent) = asset.upload.intent.as_ref() else {
            return Ok(());
        };
        if planned.is_some_and(|planned| planned != intent) {
            return Ok(());
        }
        let file_name = asset.upload.local_versions.first().map(|(_, file_name, _)| file_name.as_str()).unwrap_or("unknown file");
        let Some(book) = self.assets.verified_book_version_reader(*content_hash, intent.checksum, asset.upload_paths.clone()).await? else {
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
        let upload_required = if already_admitted {
            true
        } else {
            let admission = transfer_deadline(
                SMALL_REQUEST_TIMEOUT,
                crate::negotiate_blobs(&self.http_client, &credentials, &self.library_id, vec![sync_common::api::assets::BlobManifestEntry { content_hash: *content_hash, checksum: intent.checksum, size_bytes: intent.size_bytes }]),
            )
            .await?;
            let admission = admission.map_err(|error| match error {
                sync_transport::SyncRequestError::AuthenticationRequired => TransferError::AuthenticationRequired,
                error => TransferError::retryable(error),
            })?;
            if let Some(rejected) = admission.rejected.first() {
                return Err(TransferError::from_upload_admission(rejected.reason));
            }
            // Transport validates that every requested hash has one outcome.
            !admission.upload.is_empty()
        };
        if upload_required {
            #[cfg(not(target_arch = "wasm32"))]
            {
                transfer_deadline(
                    upload_timeout(intent.size_bytes),
                    crate::upload_book_revision_stream(
                        &self.http_client,
                        credentials.server_url(),
                        credentials.access_token(),
                        &self.library_id,
                        content_hash,
                        &intent.checksum,
                        intent.size_bytes,
                        reqwest::Body::wrap_stream(book.into_upload_stream()),
                    ),
                )
                .await?
                .map_err(TransferError::from)?;
            }
            #[cfg(target_arch = "wasm32")]
            transfer_deadline(upload_timeout(book.length), web_upload::upload(&book, &sync_transport::blob_endpoint(credentials.server_url(), &self.library_id, content_hash), credentials.access_token())).await??;
        }
        // Currency is re-validated when the orchestrator commits this record;
        // a placement that changed mid-drain is rejected there and the next
        // planning pass retries the current version.
        out.record(library_database::TransferWrite::CompleteUploadedBook { hash: *content_hash, local_versions: asset.upload.local_versions.clone(), intent: Some(intent.clone()) });
        Ok(())
    }

    pub async fn download_blob(&self, snapshot: &library_database::TransferSnapshot, content_hash: &ContentHash, placement: &BookPlacement, out: &mut library_database::TransferOutcome) -> Result<(), TransferError> {
        // No live placement expects this download: the placement went stale
        // (or never named this hash), so there is nothing to publish. This
        // mirrors the currency skip below; the queue job still completes.
        let Some(entry) = snapshot.placement_for(content_hash, &placement.rel_path) else {
            return Ok(());
        };
        #[cfg(not(target_arch = "wasm32"))]
        let Some(book) = entry.book.clone() else {
            return Ok(());
        };
        #[cfg(target_arch = "wasm32")]
        if !entry.placement_current {
            out.record(library_database::TransferWrite::CancelDownload { hash: *content_hash });
            return Ok(());
        }
        let credentials = self.credentials()?;
        self.send_download_status(*content_hash, DownloadState::Downloading(DownloadProgress::ZERO));
        let response = self.http_client.get(sync_transport::blob_endpoint(credentials.server_url(), &self.library_id, content_hash)).bearer_auth(credentials.access_token()).send().await.map_err(crate::AssetResponseError::Retryable)?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            out.record(library_database::TransferWrite::CancelDownload { hash: *content_hash });
            return Ok(());
        }
        let response = crate::validate_streaming_asset_response(response)?;
        let total_bytes = response.content_length();
        let checksum_header = response.headers().get("x-bokheim-content-checksum").ok_or_else(|| TransferError::rejected("book response is missing its transfer checksum"))?;
        let transfer_checksum = ContentHash::try_from(checksum_header.as_bytes()).map_err(|_| TransferError::rejected("invalid book transfer checksum"))?;
        self.assets.check_write_capacity(total_bytes.unwrap_or(1))?;
        #[cfg(target_arch = "wasm32")]
        let mut destination = self.assets.begin_stream_write(BlobKind::Book, content_hash, transfer_checksum).await?;
        #[cfg(not(target_arch = "wasm32"))]
        let mut destination = {
            self.assets.ensure_placement_parent(&placement.rel_path)?;
            self.assets.begin_book_write(&placement.rel_path, content_hash, transfer_checksum).await?
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
                    self.send_download_status(*content_hash, DownloadState::Downloading(progress));
                }
            }
        }
        let destination = destination.verify().await.map_err(|error| {
            out.record(library_database::TransferWrite::CancelDownload { hash: *content_hash });
            TransferError::rejected(error.to_string())
        })?;
        #[cfg(not(target_arch = "wasm32"))]
        {
            let destination = destination.finish_book().await.map_err(|error| {
                out.record(library_database::TransferWrite::CancelDownload { hash: *content_hash });
                TransferError::rejected(error.to_string())
            })?;
            let published = destination.publish_snapshot(&self.assets, &book.relative_path, &book.occupied_names, book.format == book_model::BookFormat::Mp3Folder)?;
            // Commit revalidates the placement and name conflicts before applying the result.
            out.record(library_database::TransferWrite::CompleteDownloadedBook {
                completion: NativeBookCompletion { snapshot: book, publication: published, checksum: transfer_checksum, size_bytes: completed_bytes },
                thumbnail_set_exists: self.assets.thumbnail_set_exists(content_hash)?,
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            destination.commit().await?;
            out.record(library_database::TransferWrite::CompleteLocalBookRequest { hash: *content_hash });
        }
        Ok(())
    }
}
