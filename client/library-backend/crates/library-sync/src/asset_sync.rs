//! One database-driven asset pass per library. Cursors advance before execution;
//! failed work stays durable and cannot hold later pages behind it.
use super::{error::SyncResult, library_sync::LibrarySync};
use asset_transfer::{transfer::AssetExecutor, TransferError};
use library_model::TransferJobKind;
use library_runtime::events::LibraryEvent;
use sync_common::ContentHash;

const BOOK_PAGE: i64 = sync_common::api::assets::MAX_BLOB_MANIFEST_ENTRIES as i64;
const COVER_PAGE: i64 = sync_common::api::assets::MAX_THUMBNAIL_BATCH_ENTRIES as i64;
const PRESENCE_PAGE: usize = sync_common::api::assets::MAX_THUMBNAIL_PRESENCE_ENTRIES;

impl LibrarySync {
    /// Records an ordinary failure and lets the pass continue; only a lost
    /// sign-in stops it.
    fn asset_result(kind: TransferJobKind, failed: &mut bool, result: Result<(), TransferError>) -> Result<(), TransferError> {
        match result {
            Err(TransferError::AuthenticationRequired) => Err(TransferError::AuthenticationRequired),
            Err(error) => {
                log::warn!("{kind:?} will retry on the next asset pass: {error}");
                *failed = true;
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }

    /// File inspection checks presence before extracting covers. This retains
    /// that ordering while sharing validation and credential renewal with assets.
    pub async fn record_remote_thumbnail_presence(&self, hashes: Vec<ContentHash>) -> SyncResult<()> {
        if !self.is_signed_in() {
            return Ok(());
        }
        let executor = self.asset_executor();
        for page in hashes.chunks(PRESENCE_PAGE) {
            executor.cover_presence(page).await?;
        }
        Ok(())
    }

    pub async fn run_asset_pass(&self) -> Result<(), TransferError> {
        if !self.is_signed_in() {
            return Ok(());
        }
        let executor = self.asset_executor();
        let mut failed = false;
        for kind in [
            #[cfg(not(target_arch = "wasm32"))]
            TransferJobKind::DownloadBook,
            TransferJobKind::UploadBook,
            TransferJobKind::UploadThumbnail,
            TransferJobKind::DownloadThumbnail,
        ] {
            let result = self.asset_phase(&executor, kind, &mut failed).await;
            let result = Self::asset_result(kind, &mut failed, result);
            executor.flush_availability_changes();
            result?;
        }
        if failed {
            Err(TransferError::retryable("asset pass has pending failures"))
        } else {
            Ok(())
        }
    }

    async fn asset_phase(&self, executor: &AssetExecutor, kind: TransferJobKind, failed: &mut bool) -> Result<(), TransferError> {
        match kind {
            #[cfg(not(target_arch = "wasm32"))]
            TransferJobKind::DownloadBook => {
                let (mut after_at, mut after_hash) = (-1, None);
                loop {
                    let page = self.database.book_download_requests(after_at, after_hash.as_ref(), BOOK_PAGE)?;
                    let Some((hash, at)) = page.last() else { break };
                    (after_at, after_hash) = (*at, Some(*hash));
                    for (hash, _) in page {
                        Self::asset_result(kind, failed, executor.download_book(hash).await)?;
                    }
                    tokio::task::yield_now().await;
                }
            }
            TransferJobKind::UploadBook if self.asset_storage_enabled() => {
                let mut after = 0;
                loop {
                    let page = self.database.pending_book_uploads(after, BOOK_PAGE)?;
                    let Some(last) = page.last() else { break };
                    after = last.id;
                    match executor.upload_books(&page).await {
                        Ok(results) => {
                            if results.iter().any(Result::is_ok) {
                                if let Some(events) = &self.events {
                                    let _ = events.emit(LibraryEvent::BooksUploaded);
                                }
                            }
                            for result in results {
                                Self::asset_result(kind, failed, result)?;
                            }
                        }
                        Err(error) => Self::asset_result(kind, failed, Err(error))?,
                    }
                    tokio::task::yield_now().await;
                }
            }
            TransferJobKind::UploadThumbnail if self.asset_storage_enabled() => {
                // Retain cover discovery for locally stored books as well as
                // remote books. It must precede cover upload selection.
                let mut after = None;
                loop {
                    let page = self.database.thumbnail_remote_presence_candidates(after.as_ref())?;
                    let Some(last) = page.last() else { break };
                    after = Some(*last);
                    Self::asset_result(kind, failed, executor.cover_presence(&page).await.map(|_| ()))?;
                    tokio::task::yield_now().await;
                }
                let mut after = None;
                loop {
                    let page = self.database.pending_cover_uploads(after.as_ref(), COVER_PAGE)?;
                    let Some(last) = page.last() else { break };
                    after = Some(*last);
                    match executor.upload_covers(&page).await {
                        Ok(results) => {
                            for result in results {
                                Self::asset_result(kind, failed, result)?;
                            }
                        }
                        Err(error) => Self::asset_result(kind, failed, Err(error))?,
                    }
                    tokio::task::yield_now().await;
                }
            }
            TransferJobKind::DownloadThumbnail => {
                let mut after = None;
                loop {
                    let page = self.database.pending_thumbnail_downloads(after.as_ref(), COVER_PAGE)?;
                    let Some(last) = page.last() else { break };
                    after = Some(*last);
                    Self::asset_result(kind, failed, executor.download_covers(&page).await)?;
                    tokio::task::yield_now().await;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
