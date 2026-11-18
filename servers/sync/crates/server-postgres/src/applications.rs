use crate::assets::BookUploadReservation;
use crate::assets::{self, BookUploadAdmission};
use crate::cleanup::{lock_publication, repair_unbound_aliases};
use crate::PostgresDatabase;
use content_address::ContentHash;
use server_asset_store::{AssetError, AssetKind, AssetReader, FsAssetStore, StoredAsset};
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;
use sync_common::api::assets::{BlobManifestEntry, BlobManifestResponse};
use sync_common::BookId;
use tokio::sync::Notify;

pub struct AssetApplication {
    pub(super) pool: PgPool,
    pub(super) store: Arc<FsAssetStore>,
    pub(super) cleanup: Arc<Notify>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookStorageUsage {
    pub book_id: BookId,
    pub used_bytes: u64,
}

impl AssetApplication {
    pub fn new(database: PostgresDatabase, store: Arc<FsAssetStore>) -> Self {
        Self { pool: database.pool().clone(), cleanup: database.asset_cleanup_signal(), store }
    }

    pub async fn thumbnail_presence(&self, user_id: &str, library_id: &sync_common::LibraryId, hashes: &[BookId]) -> Result<Vec<sync_common::api::assets::ThumbnailRevision>, AssetError> {
        self.require_library(user_id, library_id).await?;
        let mut present = Vec::new();
        for hash in hashes {
            if let Some(revision) = self.store.thumbnail_revision(user_id, hash).await? {
                present.push(sync_common::api::assets::ThumbnailRevision { book_id: *hash, revision });
            }
        }
        Ok(present)
    }

    pub async fn remove_thumbnail(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &BookId) -> Result<(), AssetError> {
        self.require_library(user_id, library_id).await?;
        assets::validate_thumbnail(&self.pool, user_id, library_id, hash, 0).await?;
        self.store.remove_thumbnail(user_id, hash).await
    }

    async fn require_library(&self, user_id: &str, library_id: &sync_common::LibraryId) -> Result<(), AssetError> {
        let enabled: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM libraries WHERE id=$1 AND user_id=$2 AND deleted_at IS NULL AND cloud_storage_enabled)")
            .bind(library_id.to_string()).bind(user_id).fetch_one(&self.pool).await.map_err(|error| AssetError::Storage(error.to_string()))?;
        if !enabled {
            Err(AssetError::Forbidden)
        } else {
            Ok(())
        }
    }

    pub async fn negotiate_books(&self, user_id: &str, library_id: &sync_common::LibraryId, blobs: Vec<BlobManifestEntry>) -> Result<BlobManifestResponse, AssetError> {
        self.require_library(user_id, library_id).await?;
        if blobs.len() > sync_common::api::assets::MAX_BLOB_MANIFEST_ENTRIES {
            return Err(AssetError::InvalidInput("book manifest is too large".into()));
        }

        let mut hashes = HashSet::new();
        if blobs.iter().any(|blob| !hashes.insert(blob.book_id)) {
            return Err(AssetError::InvalidInput("book manifest contains duplicate content hashes".into()));
        }
        let phase_started = Instant::now();
        // Verify and pin a temporary catalog inode before taking database
        // locks. A cancellation or rejected claim releases these temporary links.
        let mut catalog = HashMap::new();
        let mut prepared_checksums = HashSet::new();
        for blob in &blobs {
            if blob.size_bytes > 0 {
                if let Some(source) = blob.byte_checksum.or_else(|| blob.book_id.content_hash()) {
                    if prepared_checksums.insert(source) {
                        if let Some(book) = self.store.prepare_catalog_book(&BookId::from(source)).await? {
                            catalog.insert(source, book);
                        }
                    }
                }
            }
        }
        let adopted_catalog = !catalog.is_empty();
        let response = assets::negotiate_books(&self.pool, user_id, library_id, &blobs, &self.store, catalog).await?;
        if adopted_catalog { self.cleanup.notify_one(); }
        tracing::debug!(target: "sync_performance", phase = "manifest_database", elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
        Ok(response)
    }

    pub async fn storage_usage(&self, user_id: &str) -> Result<StorageUsage, AssetError> {
        assets::storage_usage(&self.pool, user_id).await
    }

    pub async fn library_storage_usage(&self, user_id: &str) -> Result<Vec<LibraryStorageUsage>, AssetError> {
        assets::library_storage_usage(&self.pool, user_id).await
    }

    pub async fn book_storage_usage(&self, user_id: &str, limit: usize) -> Result<Vec<BookStorageUsage>, AssetError> {
        assets::book_storage_usage(&self.pool, user_id, limit).await
    }

    pub async fn open_from(&self, user_id: &str, library_id: &sync_common::LibraryId, kind: AssetKind, hash: &BookId, offset: u64) -> Result<Option<StoredAsset>, AssetError> {
        let checksum = if kind == AssetKind::Book {
            let Some(checksum) = self.book_content_hash(user_id, library_id, hash).await? else {
                return Ok(None);
            };
            Some(checksum)
        } else {
            self.require_library(user_id, library_id).await?;
            let Some(size) = self.store.size(user_id, kind, hash).await? else {
                return Ok(None);
            };
            assets::validate_thumbnail(&self.pool, user_id, library_id, hash, size).await?;
            None
        };
        if let Some(asset) = self.store.open_from(user_id,kind,hash,offset).await? { return Ok(Some(asset)); }
        if let Some(checksum) = checksum {
            let canonical = BookId::from(checksum);
            if canonical != *hash {
                if let Some(mut asset) = self.store.open_from(user_id,kind,&canonical,offset).await? {
                    asset.book_id = *hash;
                    return Ok(Some(asset));
                }
            }
        }
        Ok(None)
    }

    pub async fn audiobook_track(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &BookId, index: usize) -> Result<Option<(StoredAsset, audiobook_archive::ArchivedTrack)>, AssetError> {
        let Some(asset) = self.open_from(user_id, library_id, AssetKind::Book, hash, 0).await? else { return Ok(None) };
        let track = self.store.book_tracks(&asset.book_id).await?.into_iter().nth(index);
        Ok(track.map(|track| (asset, track)))
    }

    pub async fn put_book(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &BookId, length: u64, reader: AssetReader) -> Result<(), AssetError> {
        let checksum = hash.content_hash().ok_or(AssetError::HashMismatch)?;
        self.put_book_with_checksum(user_id, library_id, hash, checksum, length, reader).await
    }

    pub async fn book_content_hash(&self, user: &str, library: &sync_common::LibraryId, book: &BookId) -> Result<Option<ContentHash>, AssetError> {
        // No row means forbidden library access; a NULL checksum means the
        // account does not own this book. Lifecycle references grant no access.
        let bytes: Option<Option<Vec<u8>>> = sqlx::query_scalar(include_str!("sql/assets/book_read_access.sql"))
            .bind(library.to_string()).bind(user).bind(book.as_bytes()).fetch_optional(&self.pool).await.map_err(storage)?;
        bytes.ok_or(AssetError::Forbidden)?.map(|bytes| ContentHash::try_from(bytes.as_slice()).map_err(storage)).transpose()
    }

    pub async fn put_book_with_checksum(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &BookId, checksum: ContentHash, length: u64, reader: AssetReader) -> Result<(), AssetError> {
        self.require_library(user_id, library_id).await?;

        // Reserve and verify actual bytes even when an object is already owned.
        let phase_started = Instant::now();
        let admission = assets::reserve_book_upload(&self.pool, user_id, library_id, hash, length, checksum).await?;
        tracing::debug!(target: "sync_performance", phase = "upload_reservation", library_id = %library_id, bytes = length, elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
        let mut cleanup = match &admission {
            BookUploadAdmission::Reserved(reservation) => Some(BookReservationCleanup::new(self.pool.clone(), reservation.clone())),
            BookUploadAdmission::AlreadyOwned => None,
        };
        let result = async {
            let staged = self.store.stage_with_checksum(user_id, AssetKind::Book, hash, Some(length), Some(checksum), reader).await?;
            repair_unbound_aliases(&self.pool, &self.store, &[*hash]).await?;
            let mut transaction = self.pool.begin().await.map_err(storage)?;
            lock_publication(&mut transaction).await?;
            let phase_started = Instant::now();
            assets::finish_book_upload(&mut transaction, user_id, library_id, hash, &admission, staged.size_bytes(), checksum).await?;
            let canonical = BookId::from(checksum);
            let retained = canonical != *hash && self.store.link_book_revision(&canonical,hash,checksum,length).await?;
            if !retained {
                let sources: Vec<Vec<u8>> = sqlx::query_scalar("SELECT content_hash FROM blob_object WHERE byte_checksum=$1 AND content_hash<>$2 ORDER BY content_hash").bind(checksum.as_bytes().as_slice()).bind(hash.as_bytes()).fetch_all(&mut *transaction).await.map_err(storage)?;
                for source in sources {
                    let source = BookId::try_from(source.as_slice()).map_err(storage)?;
                    if self.store.link_book_revision(&source, hash, checksum, length).await? { break; }
                }
            }
            if retained && self.store.size(user_id,AssetKind::Book,hash).await?.is_none() { staged.discard()?; }
            else { staged.publish().await?; }
            let result = transaction.commit().await.map_err(storage);
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

    pub async fn put_thumbnail_pair(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &BookId, bytes: &[u8], browse_bytes: &[u8]) -> Result<(), AssetError> {
        self.require_library(user_id, library_id).await?;
        let mut transaction = self.pool.begin().await.map_err(storage)?;
        lock_publication(&mut transaction).await?;
        let largest_size = bytes.len().max(browse_bytes.len()) as u64;
        assets::validate_thumbnail_upload_in_transaction(&mut transaction, user_id, library_id, hash, largest_size).await?;
        self.store.put_thumbnail_pair(user_id, hash, bytes, browse_bytes).await?;
        transaction.commit().await.map_err(storage)
    }

    pub async fn thumbnail_pair(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &BookId) -> Result<Option<(Vec<u8>, Vec<u8>)>, AssetError> {
        self.require_library(user_id, library_id).await?;
        let Some(pair) = self.store.thumbnail_pair(user_id, hash).await? else { return Ok(None) };
        assets::validate_thumbnail(&self.pool, user_id, library_id, hash, pair.0.len() as u64).await?;
        Ok(Some(pair))
    }

    pub async fn put(&self, user_id: &str, library_id: &sync_common::LibraryId, kind: AssetKind, hash: &BookId, expected_len: Option<u64>, reader: AssetReader) -> Result<(), AssetError> {
        self.require_library(user_id, library_id).await?;
        let expected_len = expected_len.ok_or(AssetError::LengthRequired)?;
        if kind.max_bytes().is_some_and(|limit| expected_len > limit) {
            return Err(AssetError::TooLarge { limit: kind.max_bytes().unwrap_or(u64::MAX) });
        }
        if kind != AssetKind::Book {
            assets::validate_thumbnail_upload(&self.pool, user_id, library_id, hash, expected_len).await?;
            let staged = self.store.stage(user_id, kind, hash, Some(expected_len), reader).await?;
            let mut transaction = self.pool.begin().await.map_err(storage)?;
            lock_publication(&mut transaction).await?;
            if kind == AssetKind::ThumbnailBrowse && self.store.size(user_id, AssetKind::Thumbnail, hash).await?.is_none() {
                return Err(AssetError::InvalidInput("browse thumbnail requires a stored source".into()));
            }
            assets::validate_thumbnail_upload_in_transaction(&mut transaction, user_id, library_id, hash, staged.size_bytes()).await?;
            staged.publish().await?;
            return transaction.commit().await.map_err(storage);
        }
        self.put_book(user_id, library_id, hash, expected_len, reader).await
    }
}

fn storage(error: impl std::fmt::Display) -> AssetError {
    AssetError::Storage(error.to_string())
}
