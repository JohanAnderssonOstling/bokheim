use crate::assets::{self, BookUploadAdmission, ThumbnailUploadAdmission};
use crate::{sync_store, PostgresDatabase};
use server_asset_store::{AssetError, AssetKind, AssetReader, FsAssetStore, StoredAsset};
use sqlx::PgPool;
use std::sync::Arc;
use sync_common::api::assets::{BlobManifestEntry, BlobManifestResponse};
use sync_common::ContentHash;
use tokio::io::AsyncReadExt;

async fn consume_upload(mut reader: AssetReader, expected_len: u64) -> Result<(), AssetError> {
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer).await.map_err(|error| AssetError::InvalidInput(error.to_string()))?;
        if count == 0 {
            break;
        }
        total = total.saturating_add(count as u64);
        if total > expected_len {
            return Err(AssetError::LengthMismatch { expected: expected_len, actual: total });
        }
    }
    if total != expected_len {
        return Err(AssetError::LengthMismatch { expected: expected_len, actual: total });
    }
    Ok(())
}

pub struct AssetApplication {
    pool: PgPool,
    store: Arc<FsAssetStore>,
}

/// Releases a thumbnail quota reservation if its upload future is cancelled,
/// including cancellation by the HTTP thumbnail deadline.
struct ThumbnailReservationCleanup {
    pool: PgPool,
    reservation: Option<crate::assets::ThumbnailUploadReservation>,
}

impl ThumbnailReservationCleanup {
    fn new(pool: PgPool, reservation: crate::assets::ThumbnailUploadReservation) -> Self {
        Self { pool, reservation: Some(reservation) }
    }

    fn disarm(&mut self) {
        self.reservation = None;
    }
}

impl Drop for ThumbnailReservationCleanup {
    fn drop(&mut self) {
        let Some(reservation) = self.reservation.take() else { return };
        let pool = self.pool.clone();
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                let _ = assets::abort_thumbnail_upload(&pool, &reservation).await;
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

    async fn require_library(&self, user_id: &str, library_id: &sync_common::LibraryId) -> Result<(), AssetError> {
        let owned = sync_store::owns_library(&self.pool, user_id, library_id).await.map_err(|error| AssetError::Storage(error.to_string()))?;
        if owned {
            Ok(())
        } else {
            Err(AssetError::Forbidden)
        }
    }

    async fn has_book_entitlement(&self, user_id: &str, hash: &ContentHash) -> Result<bool, AssetError> {
        sqlx::query_scalar(
            "SELECT EXISTS(
                 SELECT 1 FROM user_blob_charge
                 WHERE user_id=$1 AND content_hash=$2
                 UNION ALL
                 SELECT 1 FROM user_blob_reference
                 WHERE user_id=$1 AND content_hash=$2 AND reference_count > 0
             )",
        )
        .bind(user_id)
        .bind(hash.as_str())
        .fetch_one(&self.pool)
        .await
        .map_err(|error| AssetError::Storage(error.to_string()))
    }

    async fn claim_existing_book(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &ContentHash) -> Result<Option<u64>, AssetError> {
        let Some(size) = self.store.size(user_id, AssetKind::Book, hash).await? else {
            return Ok(None);
        };
        assets::claim_existing_book(&self.pool, user_id, library_id, hash, size).await?;
        Ok(Some(size))
    }

    pub async fn negotiate_books(&self, user_id: &str, library_id: &sync_common::LibraryId, blobs: Vec<BlobManifestEntry>) -> Result<BlobManifestResponse, AssetError> {
        assets::negotiate_books(&self.pool, user_id, library_id, &blobs).await
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
            if !self.has_book_entitlement(user_id, hash).await? {
                return Ok(None);
            }
            match self.claim_existing_book(user_id, library_id, hash).await {
                Ok(Some(_)) => {}
                Ok(None) | Err(AssetError::Forbidden) => return Ok(None),
                Err(error) => return Err(error),
            }
        } else {
            let Some(size) = self.store.size(user_id, kind, hash).await? else {
                return Ok(None);
            };
            assets::claim_existing_thumbnail(&self.pool, user_id, library_id, hash, size).await?;
        }
        self.store.open_from(user_id, kind, hash, offset).await
    }

    pub async fn put(&self, user_id: &str, library_id: &sync_common::LibraryId, kind: AssetKind, hash: &ContentHash, expected_len: Option<u64>, reader: AssetReader) -> Result<(), AssetError> {
        self.require_library(user_id, library_id).await?;
        let expected_len = expected_len.ok_or(AssetError::LengthRequired)?;
        if kind.max_bytes().is_some_and(|limit| expected_len > limit) {
            return Err(AssetError::TooLarge { limit: kind.max_bytes().unwrap_or(u64::MAX) });
        }
        if kind == AssetKind::Thumbnail {
            let reservation = match assets::reserve_thumbnail_upload(&self.pool, user_id, library_id, hash, expected_len).await? {
                ThumbnailUploadAdmission::AlreadyOwned => return consume_upload(reader, expected_len).await,
                ThumbnailUploadAdmission::Reserved(reservation) => reservation,
            };
            let mut cleanup = ThumbnailReservationCleanup::new(self.pool.clone(), reservation.clone());
            let stored = self.store.put(user_id, kind, hash, Some(expected_len), reader).await;
            let result = match stored {
                Ok(actual_size) => assets::finish_thumbnail_upload(&self.pool, &reservation, actual_size).await,
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                if let Err(abort_error) = assets::abort_thumbnail_upload(&self.pool, &reservation).await {
                    return Err(AssetError::Storage(format!("{error}; additionally failed to release quota reservation: {abort_error}")));
                }
                cleanup.disarm();
                return Err(error);
            }
            cleanup.disarm();
            return Ok(());
        }
        let reservation = match assets::reserve_book_upload(&self.pool, user_id, library_id, hash, expected_len).await? {
            BookUploadAdmission::AlreadyOwned => return consume_upload(reader, expected_len).await,
            BookUploadAdmission::Reserved(reservation) => reservation,
        };
        match self.store.put(user_id, kind, hash, Some(expected_len), reader).await {
            Ok(actual_size) => assets::finish_book_upload(&self.pool, &reservation, actual_size).await,
            Err(error) => {
                if let Err(abort_error) = assets::abort_book_upload(&self.pool, &reservation).await {
                    return Err(AssetError::Storage(format!("{error}; additionally failed to release quota reservation: {abort_error}")));
                }
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{consume_upload, AssetApplication};
    use crate::{assets, PostgresDatabase};
    use server_asset_store::{AssetError, AssetKind, AssetReader, FsAssetStore};
    use std::sync::Arc;
    use sync_common::api::assets::BlobManifestEntry;
    use sync_common::ContentHash;

    fn upload(bytes: Vec<u8>) -> AssetReader {
        Box::new(std::io::Cursor::new(bytes))
    }

    #[tokio::test]
    async fn deduplicated_uploads_are_consumed_and_length_checked() {
        assert!(consume_upload(upload(vec![0; 128 * 1024]), 128 * 1024).await.is_ok());
        assert!(matches!(consume_upload(upload(vec![0; 9]), 8).await, Err(AssetError::LengthMismatch { .. })));
        assert!(matches!(consume_upload(upload(vec![0; 7]), 8).await, Err(AssetError::LengthMismatch { .. })));
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn globally_stored_book_is_hidden_until_the_account_has_a_reference() {
        let Some(pool) = crate::test_support::isolated_pool("asset_application_access", 4).await else { return };
        let user_id = uuid::Uuid::new_v4().to_string();
        let library_id = uuid::Uuid::new_v4();
        sqlx::query("INSERT INTO users(id, email) VALUES ($1, $2)").bind(&user_id).bind(format!("asset-access-{}@example.com", uuid::Uuid::new_v4().simple())).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id, user_id) VALUES ($1, $2)").bind(library_id.to_string()).bind(&user_id).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO user_storage_account(user_id, quota_bytes) VALUES ($1, 1024)").bind(&user_id).execute(&pool).await.unwrap();

        let bytes = b"another account's globally deduplicated book".to_vec();
        let hash = ContentHash::new(blake3::hash(&bytes).to_hex().as_str());
        let size = bytes.len() as u64;
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(FsAssetStore::new(temp.path().join("books"), temp.path().join("thumbnails")));
        store.initialize().await.unwrap();
        store.put("owner", AssetKind::Book, &hash, Some(size), upload(bytes)).await.unwrap();
        sqlx::query("INSERT INTO blob_object(content_hash, size_bytes) VALUES ($1, $2)").bind(hash.as_str()).bind(size as i64).execute(&pool).await.unwrap();

        let application = AssetApplication::new(PostgresDatabase::from_pool(pool.clone()), store);
        assert!(application.open_from(&user_id, &library_id, AssetKind::Book, &hash, 0).await.unwrap().is_none());
        let manifest = application.negotiate_books(&user_id, &library_id, vec![BlobManifestEntry { content_hash: hash.clone(), size_bytes: size }]).await.unwrap();
        assert_eq!(manifest.upload, vec![hash.clone()]);
        assert!(manifest.claimed.is_empty());

        let mut transaction = pool.begin().await.unwrap();
        assets::apply_lifecycle_reference_deltas(&mut transaction, &user_id, [(hash.clone(), 1)]).await.unwrap();
        transaction.commit().await.unwrap();
        assert!(application.open_from(&user_id, &library_id, AssetKind::Book, &hash, 0).await.unwrap().is_some());
    }
}
