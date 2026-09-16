//! Host transfer coordination. Browser ownership survives requesting-tab loss;
//! native execution is owned by the application's transfer queue.
use super::*;
use asset_transfer::ClaimedTransferJob;
use library_replica::BlobKind;

#[cfg(not(target_arch = "wasm32"))]
impl TransferWorker {
    pub(super) async fn upload_claimed_books_with_refresh(&self, claimed: &[ClaimedTransferJob]) -> Result<Vec<Result<(), TransferError>>, TransferError> {
        if !self.asset_storage_enabled() {
            return Ok(claimed.iter().map(|_| Ok(())).collect());
        }
        let _interest = self.notification_interest.work();
        let intents = claimed
            .iter()
            .map(|item| match &item.job.operation {
                TransferOperation::UploadBlob { intent: Some(intent), negotiated: true, .. } => Ok(intent.clone()),
                _ => Err(TransferError::retryable("invalid upload batch job")),
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.with_refresh(super::RefreshScope::Planned, || async { self.publication_transfers().upload_batch(&intents).await }).await
    }

    pub(crate) async fn handle_job(&self, job: TransferJob) -> Result<(), TransferError> {
        self.execute_job(job).await
    }
}

#[cfg(target_arch = "wasm32")]
impl TransferWorker {
    pub(super) async fn upload_claimed_books_with_refresh(&self, claimed: &[ClaimedTransferJob]) -> Result<Vec<Result<(), TransferError>>, TransferError> {
        asset_transfer::runner::TransferHost::execute_uploads_sequentially(self, claimed).await
    }

    pub(crate) async fn handle_job(&self, job: TransferJob) -> Result<(), TransferError> {
        if let TransferOperation::DownloadBlob { content_hash, .. } = &job.operation {
            let credentials = self.credentials()?;
            let user = client_library::sync_state::get_sync_metadata(&*self.db_pool.get()?, "bound_user_id")?.unwrap_or_default();
            let key = format!("{}|{}|{}|download|{}", credentials.server_url(), user, self.library_id, content_hash);
            for attempt in 0..2 {
                let worker = self.clone();
                let owned_job = job.clone();
                let expected_user = user.clone();
                let original = job.clone();
                // The coordinator retains the executor even if one requesting tab disconnects.
                let run = move || -> crate::browser_adapter::BytesFuture {
                    Box::pin(async move {
                        let result = async {
                            if client_library::sync_state::get_sync_metadata(&*worker.db_pool.get()?, "bound_user_id")?.unwrap_or_default() != expected_user {
                                return Err(TransferError::retryable("account changed before download execution"));
                            }
                            worker.execute_job(owned_job).await
                        }
                        .await;
                        let abandoned = if result.is_ok() {
                            if let TransferOperation::DownloadBlob { content_hash, placement } = &original.operation {
                                worker.db_pool.get().ok().and_then(|conn| client_library::assets::book_placement_expects_hash(&conn, placement, content_hash).ok()).is_some_and(|live| !live)
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        rmp_serde::to_vec_named(&(result, abandoned)).map_err(|error| error.to_string())
                    })
                };
                let response = crate::browser_adapter::bridge::transfer_request(key.clone(), run).map_err(|error| TransferError::retryable(format!("transfer coordinator unavailable: {error:?}")))?;
                let bytes = response.await.map_err(TransferError::retryable)?;
                let (result, abandoned): (Result<(), TransferError>, bool) = rmp_serde::from_slice(&bytes).map_err(TransferError::retryable)?;
                result?;
                if client_library::sync_state::get_sync_metadata(&*self.db_pool.get()?, "bound_user_id")?.unwrap_or_default() != user {
                    return Err(TransferError::retryable("account changed during shared download"));
                }
                if let TransferOperation::DownloadBlob { content_hash, placement } = &job.operation {
                    // The first executor may have lost its placement during HTTP.
                    // A follower with a still-live placement gets one fresh pass.
                    if abandoned && !self.assets.exists(BlobKind::Book, content_hash)? && client_library::assets::book_placement_expects_hash(&*self.db_pool.get()?, placement, content_hash)? {
                        if attempt == 0 {
                            continue;
                        }
                        return Err(TransferError::retryable("shared download finished without publication bytes"));
                    }
                }
                return Ok(());
            }
            unreachable!("shared download attempts return or exhaust with an error");
        }
        self.execute_job(job).await
    }
}
