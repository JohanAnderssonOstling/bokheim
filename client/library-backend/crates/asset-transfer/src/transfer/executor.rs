//! Transfer execution on snapshots, recording durable effects for commit.
//!
//! Nothing here touches storage: every method reads its inputs from the
//! drain snapshot and pushes writes into the outcome the driver commits.
//! Network negotiation stays pure data; guards re-validate at commit time.

use std::collections::HashSet;
use crate::books::BookTransfers;
use crate::planning::{BookUploadNegotiation, ThumbnailAvailabilityNegotiation, TransferPlanner};
use crate::thumbnails::ThumbnailTransfers;
use crate::TransferJob;
pub(crate) use super::worker::TransferWorker;
pub(crate) use crate::runner::{DroppedUpload, PreparedUploads};
pub(crate) use crate::ClaimedTransferJob;
pub(crate) use crate::{TransferError, TransferOperation};
pub(crate) use library_database::{TransferOutcome, TransferSnapshot, TransferWrite};
#[cfg(target_arch = "wasm32")]
pub(crate) use library_model::BlobKind;
pub(crate) use library_model::BookUploadIntent;
pub(crate) use library_model::DownloadState;
pub(crate) use library_model::TransferJobKind;
pub(crate) use library_model::TransferOrigin;
pub(crate) use library_runtime::events::LibraryEvent;
pub(crate) use sync_common::ContentHash;

use crate::deadline::transfer_deadline;
use crate::policy::SMALL_REQUEST_TIMEOUT;

impl TransferWorker {
    /// Negotiate one bounded planner page over pure intent data, then record
    /// the manifest outcome for commit. Job selection below mirrors the
    /// commit's guarded settle: present remotely completes, rejected
    /// completes with its reason, everything else transfers.
    pub(crate) async fn prepare_book_uploads(&self, intents: &[BookUploadIntent], out: &mut TransferOutcome) -> Result<PreparedUploads, TransferError> {
        if !self.asset_storage_enabled() {
            return Ok(PreparedUploads { jobs: Vec::new(), dropped: Vec::new() });
        }
        let credentials = self.credentials()?;
        let planner = self.transfer_planner();
        let negotiation: Result<BookUploadNegotiation, TransferError> = async {
            let credentials = (planner.credentials)()?;
            let manifest = intents.iter().map(|job| sync_common::api::assets::BlobManifestEntry { content_hash: job.content_hash, checksum: job.checksum, size_bytes: job.size_bytes }).collect();
            let response = transfer_deadline(SMALL_REQUEST_TIMEOUT, crate::negotiate_blobs(&planner.http_client, &credentials, &planner.library_id, manifest)).await?.map_err(|error| match error {
                sync_transport::SyncRequestError::AuthenticationRequired => TransferError::AuthenticationRequired,
                error => TransferError::retryable(error),
            })?;
            let present_remotely = response.owned.iter().chain(&response.claimed).copied().collect::<Vec<_>>();
            let rejected = response
                .rejected
                .iter()
                .filter_map(|rejection| match TransferError::from_upload_admission(rejection.reason) {
                    TransferError::Rejected(reason) => Some((rejection.content_hash, reason)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let upload_required = response.upload.into_iter().collect::<HashSet<_>>();
            Ok(BookUploadNegotiation { present_remotely, rejected, upload_required })
        }
        .await;
        let negotiation = negotiation?;
        planner.check_transfer_account(&credentials)?;
        out.record(TransferWrite::SettleManifest { intents: intents.to_vec(), present_remotely: negotiation.present_remotely.clone(), rejected: negotiation.rejected.iter().map(|(hash, error)| (*hash, error.to_string())).collect() });
        let mut jobs = Vec::new();
        let mut dropped = Vec::new();
        for intent in intents {
            if negotiation.present_remotely.contains(&intent.content_hash) {
                dropped.push(DroppedUpload::Completed { hash: intent.content_hash });
            } else if let Some((_, reason)) = negotiation.rejected.iter().find(|(hash, _)| *hash == intent.content_hash) {
                dropped.push(DroppedUpload::Rejected { hash: intent.content_hash, reason: reason.to_string() });
            } else {
                jobs.push(TransferJob::upload_book_intent(intent.clone(), negotiation.upload_required.contains(&intent.content_hash), TransferOrigin::Background));
            }
        }
        Ok(PreparedUploads { jobs, dropped })
    }

    pub async fn prepare_thumbnail_uploads_with_refresh(&self, snapshot: &TransferSnapshot, hashes: &[ContentHash], out: &mut TransferOutcome) -> Result<Vec<TransferJob>, TransferError> {
        if !self.asset_storage_enabled() {
            return Ok(Vec::new());
        }
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let guard = self.refresh_guard(super::RefreshScope::Planned)?;
        let result = self.prepare_thumbnail_uploads(snapshot, hashes, out).await;
        if !matches!(result, Err(TransferError::AuthenticationRequired)) {
            return result;
        }
        self.refresh_after_auth(guard).await?;
        self.prepare_thumbnail_uploads(snapshot, hashes, out).await
    }

    /// Check remote availability before publishing upload activity. The
    /// existing bounded thumbnail endpoint also works with older servers.
    ///
    /// Snapshot in, commit out: availability is negotiated over the network
    /// as pure data, then committed once. Upload eligibility below mirrors
    /// the commit's guarded settle over the same snapshot.
    pub(crate) async fn prepare_thumbnail_uploads(&self, snapshot: &TransferSnapshot, hashes: &[ContentHash], out: &mut TransferOutcome) -> Result<Vec<TransferJob>, TransferError> {
        if !self.asset_storage_enabled() {
            return Ok(Vec::new());
        }
        let credentials = self.credentials()?;
        let planner = self.transfer_planner();
        let negotiation: Result<ThumbnailAvailabilityNegotiation, TransferError> = async {
            let credentials = (planner.credentials)()?;
            let response = transfer_deadline(SMALL_REQUEST_TIMEOUT, sync_transport::thumbnail_presence(&planner.http_client, &credentials, &planner.library_id, hashes.to_vec())).await?.map_err(|error| match error {
                sync_transport::SyncRequestError::AuthenticationRequired => TransferError::AuthenticationRequired,
                error => TransferError::retryable(error),
            })?;
            let requested: HashSet<_> = hashes.iter().copied().collect();
            let mut answered = HashSet::new();
            for hash in &response.present {
                if !requested.contains(hash) || !answered.insert(*hash) {
                    return Err(TransferError::retryable("thumbnail availability response is invalid"));
                }
            }
            let available = response.present;
            let missing = requested.difference(&answered).copied().collect();
            Ok(ThumbnailAvailabilityNegotiation { available, missing })
        }
        .await;
        let negotiation = negotiation?;
        planner.check_transfer_account(&credentials)?;
        out.record(TransferWrite::SettleAvailability { available: negotiation.available.clone(), missing: negotiation.missing.clone() });
        Ok(negotiation.missing.iter().filter(|hash| snapshot.asset_for(hash).is_some_and(|asset| asset.thumbnail_allowed)).map(|hash| TransferJob::upload_thumbnail(*hash, TransferOrigin::Background)).collect())
    }

    pub(crate) async fn handle_claimed_batch(&self, snapshot: &TransferSnapshot, claimed: &[ClaimedTransferJob], out: &mut TransferOutcome) -> Result<(), TransferError> {
        #[cfg(target_arch = "wasm32")]
        if let Some(item) = claimed.first().filter(|item| matches!(item.job.operation, TransferOperation::DownloadBlob { .. })) {
            // Scheduler-owned downloads retain cross-tab ownership and guarded commits.
            return self.handle_job(item.job.clone()).await;
        }
        match claimed.first().map(|item| item.job.batch_kind()) {
            Some(Some(TransferJobKind::DownloadThumbnail)) => self.thumbnail_transfers().download_thumbnail_batch(snapshot, claimed, out).await,
            Some(Some(TransferJobKind::UploadThumbnail)) => {
                if !self.asset_storage_enabled() {
                    return Ok(());
                }
                self.thumbnail_transfers().upload_thumbnail_batch(snapshot, claimed, out).await
            }
            Some(_) => self.execute_job(snapshot, claimed[0].job.clone(), out).await,
            None => Ok(()),
        }
    }

    pub(crate) async fn execute_job(&self, snapshot: &TransferSnapshot, job: TransferJob, out: &mut TransferOutcome) -> Result<(), TransferError> {
        match job.operation {
            TransferOperation::UploadBlob { content_hash, intent, negotiated } => {
                if !self.asset_storage_enabled() {
                    return Ok(());
                }
                self.book_transfers().upload_blob_version(snapshot, &content_hash, intent.as_ref(), negotiated, out).await
            }
            TransferOperation::DownloadBlob { content_hash, placement } => {
                // A hash-addressed store can satisfy another placement from
                // retained bytes. Native stores must materialize the
                // requested filesystem placement instead, so only the
                // browser path completes here; anything else downloads.
                // Currency re-validates when this record commits.
                #[cfg(target_arch = "wasm32")]
                if placement.content_hash == content_hash && self.assets.exists(BlobKind::Book, &content_hash)? {
                    if snapshot.placement_for(&content_hash, &placement.rel_path).is_some_and(|entry| entry.placement_current) {
                        out.record(TransferWrite::CompleteRetainedDownload { hash: content_hash, rel_path: placement.rel_path.clone() });
                        return Ok(());
                    }
                }
                let result = transfer_deadline(crate::policy::BOOK_DOWNLOAD_TIMEOUT, self.book_transfers().download_blob(snapshot, &content_hash, &placement, out)).await.and_then(|result| result);
                if result.is_err() {
                    self.send_download_status(content_hash, DownloadState::Queued);
                }
                result
            }
            TransferOperation::UploadThumbnail { thumbnail_hash, .. } => {
                if !self.asset_storage_enabled() {
                    return Ok(());
                }
                self.thumbnail_transfers().upload_thumbnail(snapshot, &thumbnail_hash, out).await
            }
            TransferOperation::DownloadThumbnail { .. } => Err(TransferError::rejected("thumbnail download job bypassed batch execution")),
        }
    }

    pub(crate) fn credentials(&self) -> Result<sync_transport::SyncCredentials, TransferError> {
        self.credentials.credentials().ok_or(TransferError::AuthenticationRequired)
    }

    pub(crate) fn check_transfer_account(&self, credentials: &sync_transport::SyncCredentials) -> Result<(), TransferError> {
        if self.credentials()?.server_url() != credentials.server_url() {
            return Err(TransferError::retryable("credentials changed during asset transfer"));
        }
        Ok(())
    }

    pub(crate) fn transfer_planner(&self) -> TransferPlanner<impl Fn() -> Result<sync_transport::SyncCredentials, TransferError> + Sync + '_> {
        // Borrow only the credentials field: the closure must stay Sync, and
        // storage never crosses into it.
        let credentials = &self.credentials;
        TransferPlanner { http_client: self.http_client.clone(), library_id: self.library_id, credentials: move || credentials.credentials().ok_or(TransferError::AuthenticationRequired) }
    }
    pub(crate) fn thumbnail_transfers(
        &self,
    ) -> ThumbnailTransfers<
        impl Fn() -> Result<sync_transport::SyncCredentials, TransferError> + Sync + '_,
        impl Fn(Vec<u8>, u32) -> client_platform_runtime::executor::BoxedBackendFuture<'static, Result<Vec<u8>, client_runtime::BackendError>> + Sync,
        impl Fn(ContentHash) + Sync + '_,
    > {
        // Borrow only shareable fields: these closures must stay Sync, and
        // storage never crosses into them.
        let credentials = &self.credentials;
        let event_tx = &self.event_tx;
        let cpu = self.cpu.clone();
        ThumbnailTransfers {
            assets: self.assets.clone(),
            http_client: self.http_client.clone(),
            library_id: self.library_id,
            credentials: move || credentials.credentials().ok_or(TransferError::AuthenticationRequired),
            resize: move |bytes, width| -> client_platform_runtime::executor::BoxedBackendFuture<'static, Result<Vec<u8>, client_runtime::BackendError>> {
                let cpu = cpu.clone();
                Box::pin(async move { cpu_host::resize(&*cpu, bytes, width).await })
            },
            ready: move |content_hash| {
                if let Some(tx) = event_tx {
                    let _ = tx.try_send(LibraryEvent::ThumbnailGenerated { content_hash });
                }
            },
        }
    }

    pub(crate) fn book_transfers(&self) -> BookTransfers<impl Fn() -> Result<sync_transport::SyncCredentials, TransferError> + Sync + '_, impl Fn(ContentHash, DownloadState) + Sync + '_> {
        // Borrow only shareable fields: these closures must stay Sync, and
        // storage never crosses into them.
        let credentials = &self.credentials;
        let availability_changed = &self.availability_changed;
        let event_tx = &self.event_tx;
        #[cfg(target_arch = "wasm32")]
        let progress_key = &self.progress_key;
        BookTransfers {
            assets: self.assets.clone(),
            http_client: self.http_client.clone(),
            library_id: self.library_id,
            credentials: move || credentials.credentials().ok_or(TransferError::AuthenticationRequired),
            status: move |hash, state| {
                #[cfg(target_arch = "wasm32")]
                if let (Some(key), DownloadState::Downloading(progress)) = (progress_key.as_deref(), &state) {
                    client_platform_web::transport::bridge::report_transfer_progress(key, progress.value() as f64);
                }
                Self::report_download_status(availability_changed, event_tx, hash, state);
            },
        }
    }

    /// Reports one book's availability to whoever is showing that book.
    ///
    /// Downloaded and cleared books also change the counts folders display, but
    /// that is a property of the batch rather than of each book in it: a run
    /// that fetches four hundred books changes those counts once. The batch
    /// boundary announces it once after the queue drains.
    pub(crate) fn send_download_status(&self, content_hash: ContentHash, state: DownloadState) {
        Self::report_download_status(&self.availability_changed, &self.event_tx, content_hash, state);
    }

    /// Shareable reporting for transfer callbacks: touches only atomics and
    /// channels so the closure stays Sync without storage crossing into it.
    pub(crate) fn report_download_status(availability_changed: &std::sync::atomic::AtomicBool, event_tx: &Option<library_runtime::events::LibraryEventSender>, content_hash: ContentHash, state: DownloadState) {
        if matches!(state, DownloadState::Downloaded | DownloadState::NotDownloaded) {
            availability_changed.store(true, std::sync::atomic::Ordering::Release);
        }
        if let Some(tx) = event_tx {
            let _ = tx.try_send(LibraryEvent::DownloadStatusChanged { content_hash, state });
        }
    }
}
