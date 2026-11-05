//! Executes durable thumbnail recovery without application handles or UI events.

use book_access::BoxedBookReader;
use book_model::BookFormat;
use client_platform_runtime::executor::BoxedBackendFuture;
use client_protocol::BackendError;
use library_database::DatabaseClient;
use library_files::asset_store::{AssetStore, AssetStoreError};
use library_model::{BlobKind, RelativeBookPath};
use std::io::SeekFrom;
use std::sync::Arc;
use sync_common::BookId;
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
    host: Arc<dyn cpu_host::CpuHost>,
}
impl ThumbnailExtractor {
    pub fn new(host: Arc<dyn cpu_host::CpuHost>) -> Self {
        Self { host }
    }
}
impl ThumbnailProcessor for ThumbnailExtractor {
    fn decode(&self, bytes: Vec<u8>) -> BoxedBackendFuture<'_, Result<ThumbnailVersions, BackendError>> {
        Box::pin(cpu_host::cover(&*self.host, bytes))
    }
    fn generate(&self, extension: String, reader: BoxedBookReader) -> BoxedBackendFuture<'_, Result<Option<ThumbnailVersions>, BackendError>> {
        self.host.generate_thumbnail(extension, cpu_host::as_cpu_reader(reader))
    }
}

/// Receives progress from a thumbnail pass on its caller's executor.
pub trait ThumbnailObserver {
    fn completed(&mut self);
    fn ready(&mut self, hash: BookId);
}

/// Owned inputs for an independent job. It writes thumbnail files and then
/// notifies its consumer; it has no database or library-operation capability.
pub struct ThumbnailJob {
    hash: BookId,
    assets: AssetStore,
    format: BookFormat,
    paths: Vec<RelativeBookPath>,
}

impl ThumbnailJob {
    /// None reports an empty legacy source; it must not become an enrichment
    /// candidate. A missing source simply waits for a later discovery event.
    pub async fn run(self, processor: &impl ThumbnailProcessor, notify: impl FnOnce(Option<bool>)) -> Result<bool, BackendError> {
        let cached = self.assets.read_bytes(BlobKind::Thumbnail, &self.hash).await.map_err(BackendError::operation)?;
        let mut versions = match cached.as_ref() {
            Some(bytes) => processor.decode(bytes.clone()).await.ok(),
            None => None,
        };
        let invalid_source = versions.is_none();
        if versions.is_none() {
            // Source I/O also belongs to the independent job. The file lease
            // protects opening a stable reader, without taking the library gate.
            let (mut reader, sidecar) = {
                let _lease = self.assets.lease_book(&self.hash).await.map_err(BackendError::operation)?;
                let Some(reader) = self.assets.open_book_reader(&self.hash, self.paths.clone()).await.map_err(BackendError::operation)? else { return Ok(false) };
                let sidecar = if self.format == BookFormat::M4b { self.assets.m4b_sidecar_cover(self.paths).await.map_err(BackendError::operation)? } else { None };
                (reader, sidecar)
            };
            if let Some(bytes) = sidecar {
                versions = processor.decode(bytes).await.ok();
            }
            if versions.is_none() {
                if reader.seek(SeekFrom::End(0))? == 0 {
                    notify(None);
                    return Ok(true);
                }
                reader.seek(SeekFrom::Start(0))?;
                versions = processor.generate(self.format.canonical_extension().to_owned(), reader).await?;
            }
        }
        if let Some(versions) = versions.as_ref() {
            self.assets.write_generated_thumbnail(&self.hash, versions).await.map_err(BackendError::operation)?;
        }
        if invalid_source {
            if let Some(bytes) = cached.as_ref() {
                self.assets.discard_invalid_thumbnail_source(&self.hash, bytes).await.map_err(BackendError::operation)?;
            }
        }
        notify(Some(versions.is_some()));
        Ok(true)
    }
}

pub struct ThumbnailJobs<'a, P> {
    db: &'a DatabaseClient,
    assets: &'a AssetStore,
    processor: &'a P,
}

impl<'a, P: ThumbnailProcessor> ThumbnailJobs<'a, P> {
    pub fn new(db: &'a DatabaseClient, assets: &'a AssetStore, processor: &'a P) -> Self {
        Self { db, assets, processor }
    }
    /// Makes covers for these books. A failure leaves the book's work pending
    /// for a later pass; the first failure is returned once all were tried.
    pub async fn run(&self, hashes: Vec<BookId>, observer: &mut impl ThumbnailObserver) -> Result<(), BackendError> {
        let mut failure = None;
        for hash in hashes {
            match self.recover(hash, observer).await {
                Ok(true) => {
                    observer.completed();
                }
                Ok(false) => {}
                Err(error) => {
                    let title = self.db.browse_book_detail(&hash).await.ok().map(|detail| detail.book.title).unwrap_or_else(|| "Untitled".into());
                    log::warn!("thumbnail recovery for {:?} will retry: {error}", title);
                    failure.get_or_insert(error);
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }

    pub async fn recover(&self, hash: BookId, observer: &mut impl ThumbnailObserver) -> Result<bool, BackendError> {
        if self.db.thumbnail_book_format(hash).await?.is_none() {
            return Ok(false);
        }
        let Some(job) = self.prepare(hash).await? else {
            return Ok(self.db.thumbnail_work_state(&hash).await?.as_deref() != Some("pending"));
        };
        let mut result = None;
        job.run(self.processor, |has_cover| result = Some(has_cover)).await?;
        let Some(has_cover) = result else { return Ok(false) };
        if self.db.thumbnail_book_format(hash).await?.is_none() {
            return Ok(false);
        }
        if let Some(has_cover) = has_cover {
            self.db.complete_thumbnail_work(&hash, has_cover).await?;
            if has_cover {
                observer.ready(hash);
            }
        } else {
            self.db.discard_thumbnail_work(&hash).await?;
        }
        Ok(true)
    }

    /// Capture policy and file locations while the storage owner has admission.
    /// Source reads, CPU work and thumbnail writes all happen inside the job.
    pub async fn prepare(&self, hash: BookId) -> Result<Option<ThumbnailJob>, BackendError> {
        let Some(format) = self.db.thumbnail_book_format(hash).await? else { return Ok(None) };
        if self.db.server_has_cover(&hash).await? {
            self.db.discard_thumbnail_work(&hash).await?;
            return Ok(None);
        }
        Ok(Some(ThumbnailJob { hash, assets: self.assets.clone(), format, paths: self.db.book_paths(&hash).await? }))
    }
}

/// Publish the master and both card densities before reporting a ready thumbnail.
pub async fn store_thumbnail(assets: &AssetStore, hash: BookId, browse: &[u8], card_high_density: &[u8], high_density: &[u8]) -> Result<(), AssetStoreError> {
    assets.write_thumbnail_set(&hash, high_density, browse, card_high_density).await
}
