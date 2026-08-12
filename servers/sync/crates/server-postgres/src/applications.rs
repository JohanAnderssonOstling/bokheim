use crate::assets::{self, BookUploadAdmission};
use crate::{PostgresDatabase, sync_store};
use server_asset_store::{AssetError, AssetKind, AssetReader, FsAssetStore, StoredAsset};
use sqlx::PgPool;
use std::sync::Arc;
use sync_common::ContentHash;
use sync_common::api::assets::{BlobManifestEntry, BlobManifestResponse};
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
        if owned { Ok(()) } else { Err(AssetError::Forbidden) }
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
        if kind == AssetKind::Book && self.claim_existing_book(user_id, library_id, hash).await?.is_none() {
            return Ok(None);
        }
        self.store.open_from(user_id, kind, hash, offset).await
    }

    pub async fn put(&self, user_id: &str, library_id: &sync_common::LibraryId, kind: AssetKind, hash: &ContentHash, expected_len: Option<u64>, reader: AssetReader) -> Result<(), AssetError> {
        self.require_library(user_id, library_id).await?;
        if kind == AssetKind::Thumbnail {
            self.store.put(user_id, kind, hash, expected_len, reader).await?;
            return Ok(());
        }
        let expected_len = expected_len.ok_or(AssetError::LengthRequired)?;
        if kind.max_bytes().is_some_and(|limit| expected_len > limit) {
            return Err(AssetError::TooLarge { limit: kind.max_bytes().unwrap_or(u64::MAX) });
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
    use super::consume_upload;
    use server_asset_store::{AssetError, AssetReader};

    fn upload(bytes: Vec<u8>) -> AssetReader {
        Box::new(std::io::Cursor::new(bytes))
    }

    #[tokio::test]
    async fn deduplicated_uploads_are_consumed_and_length_checked() {
        assert!(consume_upload(upload(vec![0; 128 * 1024]), 128 * 1024).await.is_ok());
        assert!(matches!(consume_upload(upload(vec![0; 9]), 8).await, Err(AssetError::LengthMismatch { .. })));
        assert!(matches!(consume_upload(upload(vec![0; 7]), 8).await, Err(AssetError::LengthMismatch { .. })));
    }
}
