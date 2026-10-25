//! One database-driven asset pass per library. Cursors advance before execution;
//! failed work stays durable and cannot hold later pages behind it.
use super::{error::SyncResult, library_sync::LibrarySync};
use asset_transfer::{TransferError, transfer::AssetExecutor};
use library_model::TransferJobKind;
use library_runtime::events::LibraryEvent;
use sync_common::BookId;

const BOOK_PAGE: i64 = sync_common::api::assets::MAX_BLOB_MANIFEST_ENTRIES as i64;
const COVER_PAGE: i64 = sync_common::api::assets::MAX_THUMBNAIL_BATCH_ENTRIES as i64;
const PRESENCE_PAGE: usize = sync_common::api::assets::MAX_THUMBNAIL_PRESENCE_ENTRIES;

#[cfg(not(target_arch = "wasm32"))]
async fn download_page<F, Fut>(hashes: impl IntoIterator<Item = BookId>, download: F) -> Result<bool, TransferError>
where
    F: FnMut(BookId) -> Fut,
    Fut: std::future::Future<Output = Result<(), TransferError>>,
{
    use futures_util::{StreamExt, stream};
    // Poll on the library thread; no connection or transaction crosses threads.
    let mut downloads = stream::iter(hashes.into_iter().map(download)).buffer_unordered(4);
    let mut failed = false;
    while let Some(result) = downloads.next().await {
        LibrarySync::asset_result(TransferJobKind::DownloadBook, &mut failed, result)?;
    }
    Ok(failed)
}

impl LibrarySync {
    /// Records an ordinary failure and lets the pass continue; only a lost
    /// sign-in stops it.
    fn asset_result(kind: TransferJobKind, failed: &mut bool, result: Result<(), TransferError>) -> Result<(), TransferError> {
        match result {
            Err(TransferError::AuthenticationRequired) => Err(TransferError::AuthenticationRequired),
            Err(error) => {
                log::warn!("{kind:?} failed: {error}");
                *failed = true;
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }

    /// File inspection checks presence before extracting covers. This retains
    /// that ordering while sharing validation and credential renewal with assets.
    pub async fn record_remote_thumbnail_presence(&self, hashes: Vec<BookId>) -> SyncResult<()> {
        if self.database.storage_policy()?.external().is_some() { return Ok(()); }
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
        // External bytes are handled by the library's provider worker. Even
        // thumbnail presence discovery must never hit Bokheim asset endpoints.
        if self.database.storage_policy()?.external().is_some() { return Ok(()); }
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
        if failed { Err(TransferError::retryable("asset pass has pending failures")) } else { Ok(()) }
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
                    *failed |= download_page(page.into_iter().map(|(hash, _)| hash), |hash| executor.download_book(hash)).await?;
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::{cell::Cell, future::pending, rc::Rc};
    use tokio::sync::Semaphore;

    struct Active(Rc<Cell<usize>>);

    impl Drop for Active {
        fn drop(&mut self) { self.0.set(self.0.get() - 1); }
    }

    #[tokio::test]
    async fn downloads_overlap_with_a_four_request_limit() {
        let active = Rc::new(Cell::new(0));
        let peak = Rc::new(Cell::new(0));
        let finished = Rc::new(Cell::new(0));
        let gate = Rc::new(Semaphore::new(0));
        let download = |_: BookId| {
            let (active, peak, finished, gate) = (active.clone(), peak.clone(), finished.clone(), gate.clone());
            async move {
                active.set(active.get() + 1);
                let _active = Active(active.clone());
                peak.set(peak.get().max(active.get()));
                gate.acquire().await.unwrap().forget();
                finished.set(finished.get() + 1);
                Ok(())
            }
        };
        let release = async {
            for _ in 0..100 {
                if active.get() == 4 { break; }
                tokio::task::yield_now().await;
            }
            assert_eq!(active.get(), 4);
            gate.add_permits(12);
        };
        let (result, ()) = tokio::join!(download_page((0..12).map(|index| BookId::from_bytes([index; 32])), download), release);
        assert!(!result.unwrap());
        assert_eq!(peak.get(), 4);
        assert_eq!(finished.get(), 12);
        assert_eq!(active.get(), 0);
    }

    #[tokio::test]
    async fn ordinary_download_failures_leave_later_work_running() {
        let completed = Cell::new(0);
        let failed = download_page((0..10).map(|index| BookId::from_bytes([index; 32])), |hash| {
            let completed = &completed;
            async move {
                completed.set(completed.get() + 1);
                if hash == BookId::from_bytes([1; 32]) {
                    Err(TransferError::retryable("temporary failure"))
                } else if hash == BookId::from_bytes([2; 32]) {
                    Err(TransferError::rejected("permanent failure"))
                } else { Ok(()) }
            }
        }).await.unwrap();
        assert!(failed);
        assert_eq!(completed.get(), 10);
    }

    #[tokio::test]
    async fn authentication_failure_cancels_in_flight_downloads_and_stops_the_page() {
        let started = Rc::new(Cell::new(0));
        let active = Rc::new(Cell::new(0));
        let result = download_page((0..12).map(|index| BookId::from_bytes([index; 32])), |hash| {
            let (started, active) = (started.clone(), active.clone());
            async move {
                started.set(started.get() + 1);
                active.set(active.get() + 1);
                let _active = Active(active);
                if hash == BookId::from_bytes([0; 32]) {
                    for _ in 0..100 {
                        if started.get() == 4 { break; }
                        tokio::task::yield_now().await;
                    }
                    Err(TransferError::AuthenticationRequired)
                } else { pending().await }
            }
        }).await;
        assert!(matches!(result, Err(TransferError::AuthenticationRequired)));
        assert_eq!(started.get(), 4);
        assert_eq!(active.get(), 0);
    }
}
