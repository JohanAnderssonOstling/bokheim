//! Typed asset execution on the library thread, with guarded database writes.
use crate::{books::BookTransfers, thumbnails::ThumbnailTransfers, TransferError};
#[cfg(not(target_arch = "wasm32"))]
use crate::books::report_download_status;
use crate::{deadline::transfer_deadline, policy::SMALL_REQUEST_TIMEOUT};
use library_database::Database;
use library_files::AssetStore;
use library_model::BookUploadIntent;
use library_runtime::{
    account::{LibraryAccount, SharedSyncCredentials},
    events::{LibraryEvent, LibraryEventRouter},
    interest::NotificationInterest,
};
use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use sync_common::{ContentHash, LibraryId};

pub struct AssetExecutor {
    pub asset_storage_enabled: Arc<AtomicBool>,
    pub assets: AssetStore,
    pub http_client: reqwest::Client,
    pub library_id: LibraryId,
    pub events: Option<LibraryEventRouter>,
    pub credentials: SharedSyncCredentials,
    pub account: Option<LibraryAccount>,
    pub notification_interest: NotificationInterest,
    pub availability_changed: Arc<AtomicBool>,
    pub database: Arc<Database>,
    pub cpu: Arc<dyn cpu_host::CpuHost>,
}

impl AssetExecutor {
    fn asset_storage_enabled(&self) -> bool {
        self.asset_storage_enabled.load(Ordering::Acquire)
    }

    pub(super) fn live_user_id(&self) -> Option<String> {
        self.account.as_ref()?.current().ok()??.user_id().to_owned().into()
    }

    async fn refreshed<T, F, Fut>(&self, mut operation: F) -> Result<T, TransferError>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, TransferError>>,
    {
        let guard = self.refresh_guard()?;
        match operation().await {
            Err(TransferError::AuthenticationRequired) => {
                self.refresh_after_auth(guard).await?;
                operation().await
            }
            result => result,
        }
    }

    async fn negotiate_uploads(&self, intents: &[BookUploadIntent]) -> Result<sync_common::api::assets::BlobManifestResponse, TransferError> {
        let credentials = self.credentials()?;
        let manifest = intents.iter().map(|intent| sync_common::api::assets::BlobManifestEntry { content_hash: intent.content_hash, size_bytes: intent.size_bytes }).collect();
        let response = transfer_deadline(SMALL_REQUEST_TIMEOUT, crate::negotiate_blobs(&self.http_client, &credentials, &self.library_id, manifest)).await?.map_err(|error| match error {
            sync_transport::SyncRequestError::AuthenticationRequired => TransferError::AuthenticationRequired,
            error => TransferError::retryable(error),
        })?;
        self.check_transfer_account(&credentials)?;
        Ok(response)
    }

    /// Negotiate once per page; native uploads run four at a time, browser
    /// uploads sequentially. Every book writes as soon as its request finishes.
    pub async fn upload_books(&self, intents: &[BookUploadIntent]) -> Result<Vec<Result<(), TransferError>>, TransferError> {
        if !self.asset_storage_enabled() || intents.is_empty() {
            return Ok(Vec::new());
        }
        let _interest = self.notification_interest.work();
        let response = self.refreshed(|| self.negotiate_uploads(intents)).await?;
        let present: Vec<_> = intents.iter().filter(|intent| response.owned.contains(&intent.content_hash) || response.claimed.contains(&intent.content_hash)).cloned().collect();
        self.database.settle_book_manifest(&present)?;
        let uploads = intents.iter().map(|intent| {
            let present = present.iter().any(|item| item.id == intent.id);
            let rejected = response.rejected.iter().find(|item| item.content_hash == intent.content_hash).map(|item| TransferError::from_upload_admission(item.reason));
            async move {
                let result = if let Some(error) = rejected {
                    Err(error)
                } else if present {
                    Ok(())
                } else {
                    self.refreshed(|| self.upload_book(intent)).await
                };
                result
            }
        });
        #[cfg(not(target_arch = "wasm32"))]
        let results = {
            use futures_util::StreamExt;
            let mut uploads = futures_util::stream::iter(uploads).buffer_unordered(4);
            let mut results = Vec::new();
            while let Some(result) = uploads.next().await {
                let authentication = matches!(result, Err(TransferError::AuthenticationRequired));
                results.push(result);
                if authentication {
                    break;
                }
            }
            results
        };
        #[cfg(target_arch = "wasm32")]
        let results = {
            let mut results = Vec::new();
            for upload in uploads {
                let result = upload.await;
                let authentication = matches!(result, Err(TransferError::AuthenticationRequired));
                results.push(result);
                if authentication {
                    break;
                }
            }
            results
        };
        Ok(results)
    }

    async fn upload_book(&self, intent: &BookUploadIntent) -> Result<(), TransferError> {
        if !self.asset_storage_enabled() {
            return Ok(());
        }
        let snapshot = self.database.transfer_snapshot(&[intent.content_hash])?;
        self.book_transfers().upload_blob_version(&snapshot, intent).await
    }

    /// Presence discovery also serves local extraction, which skips books with
    /// server covers. Validate the response before changing any presence facts.
    pub async fn cover_presence(&self, hashes: &[ContentHash]) -> Result<Vec<ContentHash>, TransferError> {
        self.refreshed(|| async {
            let credentials = self.credentials()?;
            let response = transfer_deadline(SMALL_REQUEST_TIMEOUT, sync_transport::thumbnail_presence(&self.http_client, &credentials, &self.library_id, hashes.to_vec())).await?.map_err(|error| match error {
                sync_transport::SyncRequestError::AuthenticationRequired => TransferError::AuthenticationRequired,
                error => TransferError::retryable(error),
            })?;
            let requested: std::collections::HashSet<_> = hashes.iter().copied().collect();
            let present: std::collections::HashSet<_> = response.present.iter().copied().collect();
            if present.len() != response.present.len() || !present.is_subset(&requested) {
                return Err(TransferError::retryable("invalid thumbnail presence response"));
            }
            self.check_transfer_account(&credentials)?;
            self.database.record_cover_presence(hashes, &response.present)?;
            Ok(hashes.iter().filter(|hash| !present.contains(hash)).copied().collect())
        })
        .await
    }

    pub async fn upload_covers(&self, hashes: &[ContentHash]) -> Result<Vec<Result<(), TransferError>>, TransferError> {
        if !self.asset_storage_enabled() {
            return Ok(Vec::new());
        }
        let _interest = self.notification_interest.work();
        let missing = self.cover_presence(hashes).await?;
        if missing.is_empty() {
            return Ok(Vec::new());
        }
        match self.refreshed(|| async { self.thumbnail_transfers().upload_thumbnail_batch(&missing).await }).await {
            Err(TransferError::Rejected(_)) if missing.len() > 1 => {
                let mut results = Vec::new();
                for hash in &missing {
                    let result = self.refreshed(|| async { self.thumbnail_transfers().upload_thumbnail_batch(std::slice::from_ref(hash)).await }).await;
                    let authentication = matches!(result, Err(TransferError::AuthenticationRequired));
                    results.push(result);
                    if authentication {
                        break;
                    }
                }
                Ok(results)
            }
            result => Ok(vec![result]),
        }
    }

    pub async fn download_covers(&self, hashes: &[ContentHash]) -> Result<(), TransferError> {
        let _interest = self.notification_interest.work();
        self.refreshed(|| async { self.thumbnail_transfers().download_thumbnail_batch(hashes).await }).await
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn download_book(&self, hash: ContentHash) -> Result<(), TransferError> {
        let _interest = self.notification_interest.work();
        let paths = self.database.book_paths(&hash)?;
        if self.assets.book_exists(&hash, &paths)? {
            self.database.complete_local_book_request(&hash)?;
        } else {
            let snapshot = self.database.transfer_snapshot(&[hash])?;
            let placement = snapshot.placements.iter().find(|entry| entry.content_hash == hash && entry.book.is_some());
            if let Some(entry) = placement {
                let placement = library_model::BookPlacement::new(hash, entry.rel_path.clone());
                let result = self.refreshed(|| async { transfer_deadline(crate::policy::BOOK_DOWNLOAD_TIMEOUT, self.book_transfers().download_blob(&snapshot, &hash, &placement)).await? }).await;
                if matches!(result, Err(TransferError::Rejected(_))) {
                    self.database.complete_download(&hash)?;
                }
                if let Err(error) = result {
                    self.publish_download_status(hash)?;
                    return Err(error);
                }
            } else {
                self.database.complete_download(&hash)?;
            }
        }
        self.publish_download_status(hash)
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn publish_download_status(&self, hash: ContentHash) -> Result<(), TransferError> {
        let present = self.assets.book_exists(&hash, &self.database.book_paths(&hash)?)?;
        let state = self.database.transfer_download_state(&hash, present)?;
        report_download_status(&self.availability_changed, self.events.as_ref(), hash, state);
        Ok(())
    }

    pub fn flush_availability_changes(&self) {
        if self.availability_changed.swap(false, Ordering::AcqRel) {
            if let Some(tx) = &self.events {
                let _ = tx.emit(LibraryEvent::ContentsChanged);
            }
        }
    }

    pub(super) fn credentials(&self) -> Result<sync_transport::SyncCredentials, TransferError> {
        self.credentials.credentials().ok_or(TransferError::AuthenticationRequired)
    }

    pub(crate) fn check_transfer_account(&self, credentials: &sync_transport::SyncCredentials) -> Result<(), TransferError> {
        if self.credentials()?.server_url() != credentials.server_url() {
            return Err(TransferError::retryable("credentials changed during asset transfer"));
        }
        Ok(())
    }

    pub(crate) fn thumbnail_transfers(&self) -> ThumbnailTransfers {
        ThumbnailTransfers {
            assets: self.assets.clone(),
            database: self.database.clone(),
            http_client: self.http_client.clone(),
            library_id: self.library_id,
            credentials: self.credentials.clone(),
            cpu: self.cpu.clone(),
            events: self.events.clone(),
        }
    }

    pub(crate) fn book_transfers(&self) -> BookTransfers {
        BookTransfers {
            assets: self.assets.clone(),
            database: self.database.clone(),
            http_client: self.http_client.clone(),
            library_id: self.library_id,
            credentials: self.credentials.clone(),
            availability_changed: self.availability_changed.clone(),
            events: self.events.clone(),
        }
    }
}
