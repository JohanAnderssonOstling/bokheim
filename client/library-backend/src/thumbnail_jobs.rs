//! Executes durable thumbnail recovery without application handles or UI events.
use library_database::{Database, DatabaseError};
use crate::executor::BoxedBackendFuture;
use crate::asset_store::{AssetStore, AssetStoreError};
use crate::{BlobKind, BoxedBookReader, ContentHash};
pub use thumbnail::ThumbnailVersions;

/// The host chooses native workers or browser CPU-worker RPC.
pub trait ThumbnailProcessor: Sync {
    fn decode(&self, bytes: Vec<u8>) -> BoxedBackendFuture<'_, Result<ThumbnailVersions, String>>;
    fn generate(&self, extension: String, reader: BoxedBookReader) -> BoxedBackendFuture<'_, Result<Option<ThumbnailVersions>, String>>;
}

/// A batch keeps its activity guard alive across pages, including cancellation.
///
/// Runs entirely on a library's own dedicated thread (see
/// `crate::executor::LibraryExecutor`), never on the shared multi-threaded
/// executor, so implementors — including ones holding a `&LibrarySession` —
/// need not be `Send`.
pub trait ThumbnailObserver {
    type Activity;
    fn prepare(&mut self, pending: u64);
    fn begin(&mut self) -> Self::Activity;
    fn completed(&mut self);
    fn ready(&mut self, hash: ContentHash);
}

#[derive(Debug)]
pub struct ThumbnailJobError(String);
impl ThumbnailJobError {
    fn operation(error: impl std::fmt::Display) -> Self {
        Self(error.to_string())
    }
}
impl std::fmt::Display for ThumbnailJobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for ThumbnailJobError {}
impl From<DatabaseError> for ThumbnailJobError {
    fn from(error: DatabaseError) -> Self {
        Self::operation(error)
    }
}
impl From<AssetStoreError> for ThumbnailJobError {
    fn from(error: AssetStoreError) -> Self {
        Self::operation(error)
    }
}

pub struct ThumbnailJobs<'a, P> {
    db: &'a Database,
    assets: &'a AssetStore,
    processor: &'a P,
}
impl<'a, P: ThumbnailProcessor> ThumbnailJobs<'a, P> {
    pub fn new(db: &'a Database, assets: &'a AssetStore, processor: &'a P) -> Self {
        Self { db, assets, processor }
    }
    pub async fn run_batch(&self, observer: &mut impl ThumbnailObserver) -> Result<(), ThumbnailJobError> {
        let pending = self.db.pending_thumbnail_job_count()?;
        observer.prepare(pending);
        let mut activity = None;
        let mut failure = None;
        let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_err(ThumbnailJobError::operation)?.as_secs() as i64;
        let mut after = String::new();
        loop {
            let cursor = after.clone();
            let page = self.db.pending_thumbnail_jobs_page(&cursor)?;
            let Some((last, _)) = page.last() else { break };
            after = last.as_str().to_owned();
            for (hash, retry_after) in page {
                if retry_after > now {
                    continue;
                }
                // One lifetime spans every file and database page in this pass.
                activity.get_or_insert_with(|| observer.begin());
                match self.recover(hash, observer).await {
                    Ok(true) => {
                        observer.completed();
                    }
                    Ok(false) => {}
                    Err(error) => {
                        log::warn!("thumbnail recovery for {hash} will retry: {error}");
                        failure.get_or_insert(error);
                    }
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }

    pub async fn recover(&self, hash: ContentHash, observer: &mut impl ThumbnailObserver) -> Result<bool, ThumbnailJobError> {
        let outcome = async {
            let _lease = self.assets.lease_book(&hash).await.map_err(ThumbnailJobError::operation)?;
            let book = self.db.thumbnail_book_format(hash)?;
            let Some(format) = book else { return Ok(false) };
            // A pending job may have left incomplete bytes behind. Decode the
            // cached master before confirming it, and rebuild its browse variant.
            let (_lease, cached) = if let Some(bytes) = self.assets.read_bytes(BlobKind::Thumbnail, &hash).await? {
                drop(_lease);
                let cached = self.processor.decode(bytes).await.ok();
                let lease = self.assets.lease_book(&hash).await.map_err(ThumbnailJobError::operation)?;
                if cached.is_none() {
                    self.assets.remove(BlobKind::Thumbnail, &hash)?;
                }
                (lease, cached)
            } else {
                (_lease, None)
            };
            if let Some(versions) = cached {
                self.store(hash, &versions, observer).await?;
                self.db.confirm_thumbnail_available(&hash)?;
                return Ok(true);
            }
            let remote = self.db.has_remote_thumbnail(&hash)?;
            if remote {
                self.db.defer_thumbnail_to_download(&hash)?;
                return Ok(true);
            }
            let Some(mut reader) = self.assets.prepare_reader(BlobKind::Book, &hash).await? else { return Ok(false) };
            if reader.seek(std::io::SeekFrom::End(0))? == 0 {
                // Retire legacy empty-file jobs without making them candidates
                // for external cover enrichment or publishing a thumbnail.
                self.db.discard_thumbnail_work(&hash)?;
                return Ok(true);
            }
            reader.seek(std::io::SeekFrom::Start(0))?;
            // The opened reader owns its bytes. Do not hold a placement lease
            // while waiting for CPU work: file moves must be able to proceed.
            drop(_lease);
            let versions = self.processor.generate(format.canonical_extension().to_owned(), reader).await.map_err(ThumbnailJobError::operation)?;
            let _lease = self.assets.lease_book(&hash).await.map_err(ThumbnailJobError::operation)?;
            if !self.db.has_book_placement(hash)? {
                return Ok(false);
            }
            let has_cover = versions.is_some();
            if let Some(versions) = versions {
                self.store(hash, &versions, observer).await?;
            }
            self.db.complete_thumbnail_work(&hash, has_cover)?;
            // Both ready and no_cover make external cover enrichment eligible.
            Ok(true)
        }
        .await;
        if outcome.is_err() {
            let _ = self.db.retry_thumbnail_work(&hash);
        }
        outcome
    }

    async fn store(&self, hash: ContentHash, versions: &ThumbnailVersions, observer: &mut impl ThumbnailObserver) -> Result<(), ThumbnailJobError> {
        store_thumbnail(self.assets, hash, &versions.browse, &versions.high_density).await?;
        observer.ready(hash);
        Ok(())
    }
}

/// Publish both derivatives before reporting a ready thumbnail.
pub async fn store_thumbnail(assets: &AssetStore, hash: ContentHash, browse: &[u8], high_density: &[u8]) -> Result<(), AssetStoreError> {
    assets.write(BlobKind::Thumbnail, &hash, high_density).await?;
    assets.write_thumbnail_variant(&hash, thumbnail::BROWSE_THUMBNAIL_WIDTH, browse).await
}

impl From<std::io::Error> for ThumbnailJobError {
    fn from(error: std::io::Error) -> Self {
        Self::operation(error)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
