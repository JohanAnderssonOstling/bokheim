//! Executes durable thumbnail recovery without application handles or UI events.
use book_access::BoxedBookReader;
use client_platform_runtime::executor::BoxedBackendFuture;
use client_runtime::BackendError;
use library_database::Database;
use library_files::asset_store::{AssetStore, AssetStoreError};
use library_replica::BlobKind;
use sync_common::ContentHash;
pub use thumbnail::ThumbnailVersions;

/// The host chooses native workers or browser CPU-worker RPC.
pub trait ThumbnailProcessor: Sync {
    fn decode(&self, bytes: Vec<u8>) -> BoxedBackendFuture<'_, Result<ThumbnailVersions, BackendError>>;
    fn generate(&self, extension: String, reader: BoxedBookReader) -> BoxedBackendFuture<'_, Result<Option<ThumbnailVersions>, BackendError>>;
}

/// Host-backed thumbnail extraction. The extractor carries an explicit host, so
/// thumbnail jobs stay platform-neutral without a process-wide default.
#[derive(Clone)]
pub struct ThumbnailExtractor {
    host: std::sync::Arc<dyn cpu_host::CpuHost>,
}
impl ThumbnailExtractor {
    pub fn new(host: std::sync::Arc<dyn cpu_host::CpuHost>) -> Self {
        Self { host }
    }
}
impl ThumbnailProcessor for ThumbnailExtractor {
    fn decode(&self, bytes: Vec<u8>) -> BoxedBackendFuture<'_, Result<ThumbnailVersions, BackendError>> {
        let host = self.host.clone();
        Box::pin(async move { cpu_host::cover(&*host, bytes).await.map(|v| ThumbnailVersions { browse: v.browse, high_density: v.high_density }) })
    }
    fn generate(&self, extension: String, reader: BoxedBookReader) -> BoxedBackendFuture<'_, Result<Option<ThumbnailVersions>, BackendError>> {
        let host = self.host.clone();
        Box::pin(async move { host.generate_thumbnail(extension, cpu_host::as_cpu_reader(reader)).await.map(|v| v.map(|v| ThumbnailVersions { browse: v.browse, high_density: v.high_density })) })
    }
}

/// A batch keeps its activity guard alive across pages, including cancellation.
///
/// Runs entirely on a library's own dedicated thread (see
/// `client_platform_runtime::executor::LibraryExecutor`), never on the shared
/// multi-threaded executor, so implementors — including ones holding a
/// `&LibrarySession` — need not be `Send`.
pub trait ThumbnailObserver {
    type Activity;
    fn prepare(&mut self, pending: u64);
    fn begin(&mut self) -> Self::Activity;
    fn completed(&mut self);
    fn ready(&mut self, hash: ContentHash);
}

pub struct ThumbnailJobs<'a, P> {
    db: &'a Database,
    assets: &'a AssetStore,
    processor: &'a P,
}

/// Selection differs; execution, progress and activity lifetime do not.
enum BatchSelection {
    Pending { after: String, now: i64 },
    Members(Vec<ContentHash>),
}

impl BatchSelection {
    fn next_page(&mut self, db: &Database) -> Result<Vec<ContentHash>, BackendError> {
        match self {
            Self::Members(hashes) => Ok(std::mem::take(hashes)),
            Self::Pending { after, now } => loop {
                let page = db.pending_thumbnail_jobs_page(after)?;
                let Some((last, _)) = page.last() else { return Ok(Vec::new()) };
                *after = last.as_str().to_owned();
                let ready = page.into_iter().filter_map(|(hash, retry_after)| (retry_after <= *now).then_some(hash)).collect::<Vec<_>>();
                // Deferred pages must not hide eligible work on later pages.
                if !ready.is_empty() {
                    return Ok(ready);
                }
            },
        }
    }
}

impl<'a, P: ThumbnailProcessor> ThumbnailJobs<'a, P> {
    pub fn new(db: &'a Database, assets: &'a AssetStore, processor: &'a P) -> Self {
        Self { db, assets, processor }
    }
    pub async fn run_batch(&self, observer: &mut impl ThumbnailObserver) -> Result<(), BackendError> {
        let pending = self.db.pending_thumbnail_job_count()?;
        observer.prepare(pending);
        let now = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map_err(BackendError::operation)?.as_secs() as i64;
        self.run_selected(BatchSelection::Pending { after: String::new(), now }, observer).await
    }

    pub async fn run_members(&self, hashes: Vec<ContentHash>, observer: &mut impl ThumbnailObserver) -> Result<(), BackendError> {
        if hashes.is_empty() {
            return Ok(());
        }
        observer.prepare(hashes.len() as u64);
        self.run_selected(BatchSelection::Members(hashes), observer).await
    }

    async fn run_selected(&self, mut selection: BatchSelection, observer: &mut impl ThumbnailObserver) -> Result<(), BackendError> {
        let mut activity = None;
        let mut failure = None;
        loop {
            let page = selection.next_page(self.db)?;
            if page.is_empty() {
                break;
            }
            for hash in page {
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

    pub async fn recover(&self, hash: ContentHash, observer: &mut impl ThumbnailObserver) -> Result<bool, BackendError> {
        let outcome = async {
            let _lease = self.assets.lease_book(&hash).await.map_err(BackendError::operation)?;
            let book = self.db.thumbnail_book_format(hash)?;
            let Some(format) = book else { return Ok(false) };
            // A pending job may have left incomplete bytes behind. Decode the
            // cached master before confirming it, and rebuild its browse variant.
            let (_lease, cached) = if let Some(bytes) = self.assets.read_bytes(BlobKind::Thumbnail, &hash).await.map_err(BackendError::operation)? {
                drop(_lease);
                let cached = self.processor.decode(bytes).await.ok();
                let lease = self.assets.lease_book(&hash).await.map_err(BackendError::operation)?;
                if cached.is_none() {
                    self.assets.remove(BlobKind::Thumbnail, &hash).map_err(BackendError::operation)?;
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
            let Some(mut reader) = self.assets.prepare_reader(BlobKind::Book, &hash).await.map_err(BackendError::operation)? else { return Ok(false) };
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
            let versions = self.processor.generate(format.canonical_extension().to_owned(), reader).await.map_err(BackendError::operation)?;
            let _lease = self.assets.lease_book(&hash).await.map_err(BackendError::operation)?;
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

    async fn store(&self, hash: ContentHash, versions: &ThumbnailVersions, observer: &mut impl ThumbnailObserver) -> Result<(), BackendError> {
        store_thumbnail(self.assets, hash, &versions.browse, &versions.high_density).await.map_err(BackendError::operation)?;
        observer.ready(hash);
        Ok(())
    }
}

/// Publish both derivatives before reporting a ready thumbnail.
pub async fn store_thumbnail(assets: &AssetStore, hash: ContentHash, browse: &[u8], high_density: &[u8]) -> Result<(), AssetStoreError> {
    assets.write(BlobKind::Thumbnail, &hash, high_density).await?;
    assets.write_thumbnail_variant(&hash, thumbnail::BROWSE_THUMBNAIL_WIDTH, browse).await
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
