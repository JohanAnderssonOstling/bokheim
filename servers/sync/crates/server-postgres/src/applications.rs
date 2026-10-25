use crate::assets::BookUploadReservation;
use crate::assets::{self, BookUploadAdmission};
use crate::{PostgresDatabase, sync_store};
use server_asset_store::{AssetError, AssetKind, AssetReader, FsAssetStore, StoredAsset};
use sqlx::PgPool;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use sync_common::ContentHash;
use sync_common::api::assets::{BlobManifestEntry, BlobManifestResponse};

pub struct AssetApplication {
    pool: PgPool,
    store: Arc<FsAssetStore>,
}

struct BookReservationCleanup {
    pool: PgPool,
    reservation: Option<BookUploadReservation>,
}

impl BookReservationCleanup {
    fn new(pool: PgPool, reservation: BookUploadReservation) -> Self {
        Self { pool, reservation: Some(reservation) }
    }

    fn disarm(&mut self) {
        self.reservation = None;
    }
}

impl Drop for BookReservationCleanup {
    fn drop(&mut self) {
        let Some(reservation) = self.reservation.take() else { return };
        let pool = self.pool.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = assets::abort_book_upload(&pool, &reservation).await;
            });
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageUsage {
    pub used_bytes: u64,
    pub reserved_bytes: u64,
    pub quota_bytes: u64,
}

/// One library's share of the charged bytes. The shares partition
/// [`StorageUsage::used_bytes`]; they are not independent per-library totals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryStorageUsage {
    pub library_id: sync_common::LibraryId,
    pub used_bytes: u64,
}

impl AssetApplication {
    pub fn new(database: PostgresDatabase, store: Arc<FsAssetStore>) -> Self {
        Self { pool: database.pool().clone(), store }
    }

    pub async fn thumbnail_presence(&self, user_id: &str, library_id: &sync_common::LibraryId, hashes: &[ContentHash]) -> Result<Vec<sync_common::api::assets::ThumbnailRevision>, AssetError> {
        self.require_library(user_id, library_id).await?;
        let mut present = Vec::new();
        for hash in hashes {
            if let Some(revision) = self.store.thumbnail_revision(user_id, hash).await? {
                present.push(sync_common::api::assets::ThumbnailRevision { content_hash: *hash, revision });
            }
        }
        Ok(present)
    }

    pub async fn remove_thumbnail(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &ContentHash) -> Result<(), AssetError> {
        assets::validate_thumbnail(&self.pool, user_id, library_id, hash, 0).await?;
        self.store.remove_thumbnail(user_id, hash).await
    }

    async fn require_library(&self, user_id: &str, library_id: &sync_common::LibraryId) -> Result<(), AssetError> {
        let owned = sync_store::owns_library(&self.pool, user_id, library_id).await.map_err(|error| AssetError::Storage(error.to_string()))?;
        if owned { Ok(()) } else { Err(AssetError::Forbidden) }
    }

    /// A book is readable only once this account has uploaded its bytes. A
    /// synchronized lifecycle reference is client-declared and grants nothing.
    async fn has_book_charge(&self, user_id: &str, hash: &ContentHash) -> Result<bool, AssetError> {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM user_blob_charge WHERE user_id=$1 AND content_hash=$2)")
            .bind(user_id)
            .bind(hash.as_bytes().as_slice())
            .fetch_one(&self.pool)
            .await
            .map_err(|error| AssetError::Storage(error.to_string()))
    }

    pub async fn negotiate_books(&self, user_id: &str, library_id: &sync_common::LibraryId, blobs: Vec<BlobManifestEntry>) -> Result<BlobManifestResponse, AssetError> {
        self.require_library(user_id, library_id).await?;
        if blobs.len() > sync_common::api::assets::MAX_BLOB_MANIFEST_ENTRIES {
            return Err(AssetError::InvalidInput("book manifest is too large".into()));
        }
        let mut hashes = HashSet::new();
        if blobs.iter().any(|blob| !hashes.insert(blob.content_hash)) {
            return Err(AssetError::InvalidInput("book manifest contains duplicate content hashes".into()));
        }
        let phase_started = Instant::now();
        let manifest_count = blobs.len();
        let mut verified = HashSet::new();
        for blob in &blobs {
            if self.store.size(user_id, AssetKind::Book, &blob.content_hash).await?.is_some() {
                verified.insert(blob.content_hash);
            }
        }
        tracing::debug!(target: "sync_performance", phase = "manifest_inspect_files", count = manifest_count, elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
        let phase_started = Instant::now();
        let response = assets::negotiate_books(&self.pool, user_id, library_id, &blobs, &verified).await?;
        tracing::debug!(target: "sync_performance", phase = "manifest_database", elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
        Ok(response)
    }

    pub async fn storage_usage(&self, user_id: &str) -> Result<StorageUsage, AssetError> {
        assets::storage_usage(&self.pool, user_id).await
    }

    pub async fn library_storage_usage(&self, user_id: &str) -> Result<Vec<LibraryStorageUsage>, AssetError> {
        assets::library_storage_usage(&self.pool, user_id).await
    }

    pub async fn open_from(&self, user_id: &str, library_id: &sync_common::LibraryId, kind: AssetKind, hash: &ContentHash, offset: u64) -> Result<Option<StoredAsset>, AssetError> {
        self.require_library(user_id, library_id).await?;
        if kind == AssetKind::Book {
            if !self.has_book_charge(user_id, hash).await? {
                return Ok(None);
            }
        } else {
            let Some(size) = self.store.size(user_id, kind, hash).await? else {
                return Ok(None);
            };
            assets::validate_thumbnail(&self.pool, user_id, library_id, hash, size).await?;
        }
        self.store.open_from(user_id, kind, hash, offset).await
    }

    pub async fn audiobook_track(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &ContentHash, index: usize) -> Result<Option<(StoredAsset, audiobook_archive::ArchivedTrack)>, AssetError> {
        let Some(asset) = self.open_from(user_id, library_id, AssetKind::Book, hash, 0).await? else { return Ok(None) };
        let track = self.store.book_tracks(&asset.content_hash).await?.into_iter().nth(index);
        Ok(track.map(|track| (asset, track)))
    }

    pub async fn put_book(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &ContentHash, length: u64, reader: AssetReader) -> Result<(), AssetError> {
        self.require_library(user_id, library_id).await?;
        // Reserve and verify actual bytes even when an object is already owned.
        let phase_started = Instant::now();
        let admission = assets::reserve_book_upload(&self.pool, user_id, library_id, hash, length).await?;
        tracing::debug!(target: "sync_performance", phase = "upload_reservation", library_id = %library_id, bytes = length, elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
        let mut cleanup = match &admission {
            BookUploadAdmission::Reserved(reservation) => Some(BookReservationCleanup::new(self.pool.clone(), reservation.clone())),
            BookUploadAdmission::AlreadyOwned => None,
        };
        let result = async {
            let actual = self.store.put(user_id, AssetKind::Book, hash, Some(length), reader).await?;
            let phase_started = Instant::now();
            let result = assets::finish_book_upload(&self.pool, user_id, library_id, hash, &admission, actual).await;
            tracing::debug!(target: "sync_performance", phase = "commit_upload_accounting", ok = result.is_ok(), elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
            result
        }
        .await;
        if result.is_err() {
            if let BookUploadAdmission::Reserved(reservation) = &admission {
                assets::abort_book_upload(&self.pool, reservation).await?;
            }
        }
        if let Some(cleanup) = &mut cleanup {
            cleanup.disarm();
        }
        result
    }

    pub async fn put_thumbnail_pair(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &ContentHash, bytes: &[u8], browse_bytes: &[u8]) -> Result<(), AssetError> {
        self.require_library(user_id, library_id).await?;
        for variant in [bytes, browse_bytes] {
            assets::validate_thumbnail_upload(&self.pool, user_id, library_id, hash, variant.len() as u64).await?;
        }
        self.store.put_thumbnail_pair(user_id, hash, bytes, browse_bytes).await?;
        // Recheck ownership and lifecycle after publishing, just as for streamed uploads.
        assets::validate_thumbnail_upload(&self.pool, user_id, library_id, hash, bytes.len() as u64).await
    }

    pub async fn thumbnail_pair(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &ContentHash) -> Result<Option<(Vec<u8>, Vec<u8>)>, AssetError> {
        self.require_library(user_id, library_id).await?;
        let Some(pair) = self.store.thumbnail_pair(user_id, hash).await? else { return Ok(None) };
        assets::validate_thumbnail(&self.pool, user_id, library_id, hash, pair.0.len() as u64).await?;
        Ok(Some(pair))
    }

    pub async fn put(&self, user_id: &str, library_id: &sync_common::LibraryId, kind: AssetKind, hash: &ContentHash, expected_len: Option<u64>, reader: AssetReader) -> Result<(), AssetError> {
        self.require_library(user_id, library_id).await?;
        let expected_len = expected_len.ok_or(AssetError::LengthRequired)?;
        if kind.max_bytes().is_some_and(|limit| expected_len > limit) {
            return Err(AssetError::TooLarge { limit: kind.max_bytes().unwrap_or(u64::MAX) });
        }
        if kind == AssetKind::ThumbnailBrowse {
            let master_size = self.store.size(user_id, AssetKind::Thumbnail, hash).await?.ok_or_else(|| AssetError::InvalidInput("browse thumbnail requires a stored source".into()))?;
            assets::validate_thumbnail(&self.pool, user_id, library_id, hash, master_size).await?;
            self.store.put(user_id, kind, hash, Some(expected_len), reader).await?;
            return Ok(());
        }
        if kind == AssetKind::Thumbnail {
            assets::validate_thumbnail_upload(&self.pool, user_id, library_id, hash, expected_len).await?;
            let actual_size = self.store.put(user_id, kind, hash, Some(expected_len), reader).await?;
            // Recheck the lifecycle after streaming, in case the book was removed.
            return assets::validate_thumbnail_upload(&self.pool, user_id, library_id, hash, actual_size).await;
        }
        self.put_book(user_id, library_id, hash, expected_len, reader).await
    }
}
