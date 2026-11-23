//! Executes durable thumbnail recovery without application handles or UI events.

use book_model::BookFormat;
use client_protocol::BackendError;
use cpu_contract::{as_cpu_reader, cover, CpuHost};
use library_database::DatabaseClient;
use library_files::asset_store::AssetStore;
use library_model::{BlobKind, RelativeBookPath};
use std::io::SeekFrom;
use sync_common::BookId;

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
    pub async fn run(self, host: &dyn CpuHost, notify: impl FnOnce(Option<bool>)) -> Result<(), BackendError> {
        let cached = self.assets.read_bytes(BlobKind::Thumbnail, &self.hash).await.map_err(BackendError::operation)?;
        let mut versions = match cached.as_ref() {
            Some(bytes) => cover(host, bytes.clone()).await.ok(),
            None => None,
        };
        let invalid_source = versions.is_none();
        if versions.is_none() {
            // Source I/O also belongs to the independent job. The file lease
            // protects opening a stable reader, without taking the library gate.
            let (mut reader, sidecar) = {
                let _lease = self.assets.lease_book(&self.hash).await.map_err(BackendError::operation)?;
                let Some(reader) = self.assets.open_book_reader(&self.hash, self.paths.clone()).await.map_err(BackendError::operation)? else { return Ok(()) };
                let sidecar = if self.format == BookFormat::M4b { self.assets.m4b_sidecar_cover(self.paths).await.map_err(BackendError::operation)? } else { None };
                (reader, sidecar)
            };
            if let Some(bytes) = sidecar {
                versions = cover(host, bytes).await.ok();
            }
            if versions.is_none() {
                if reader.seek(SeekFrom::End(0))? == 0 {
                    notify(None);
                    return Ok(());
                }
                reader.seek(SeekFrom::Start(0))?;
                versions = host.generate_thumbnail(self.format.canonical_extension().to_owned(), as_cpu_reader(reader)).await?;
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
        Ok(())
    }
}

pub struct ThumbnailJobs<'a> {
    db: &'a DatabaseClient,
    assets: &'a AssetStore,
}

impl<'a> ThumbnailJobs<'a> {
    pub fn new(db: &'a DatabaseClient, assets: &'a AssetStore) -> Self {
        Self { db, assets }
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
