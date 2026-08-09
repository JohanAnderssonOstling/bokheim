use server_asset_store::FsAssetStore;
use server_core::{AssetError, AssetKind, AssetReader, BookUploadAdmission, StoredAsset};
use std::sync::Arc;
use sync_common::api::assets::{BlobManifestEntry, BlobManifestResponse};
use sync_common::ContentHash;

use crate::PostgresSyncRepository;

pub struct AssetApplication {
    repository: Arc<PostgresSyncRepository>,
    store: Arc<FsAssetStore>,
}

impl AssetApplication {
    pub fn new(repository: Arc<PostgresSyncRepository>, store: Arc<FsAssetStore>) -> Self {
        Self { repository, store }
    }

    async fn require_library(&self, user_id: &str, library_id: &sync_common::LibraryId) -> Result<(), AssetError> {
        if self.repository.owns_library(user_id, library_id).await? {
            Ok(())
        } else {
            Err(AssetError::Forbidden)
        }
    }

    async fn claim_existing_book(&self, user_id: &str, library_id: &sync_common::LibraryId, hash: &ContentHash) -> Result<Option<u64>, AssetError> {
        let Some(size) = self.store.size(user_id, AssetKind::Book, hash).await? else {
            return Ok(None);
        };
        self.repository.claim_existing_book(user_id, library_id, hash, size).await?;
        Ok(Some(size))
    }

    pub async fn negotiate_books(&self, user_id: &str, library_id: &sync_common::LibraryId, blobs: Vec<BlobManifestEntry>) -> Result<BlobManifestResponse, AssetError> {
        self.require_library(user_id, library_id).await?;
        self.repository.negotiate_books(user_id, library_id, &blobs).await
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
        if expected_len > kind.max_bytes() {
            return Err(AssetError::TooLarge { limit: kind.max_bytes() });
        }
        if self.claim_existing_book(user_id, library_id, hash).await?.is_some() {
            return Ok(());
        }
        let reservation = match self.repository.reserve_book_upload(user_id, library_id, hash, expected_len).await? {
            BookUploadAdmission::AlreadyOwned => return Ok(()),
            BookUploadAdmission::Reserved(reservation) => reservation,
        };
        match self.store.put(user_id, kind, hash, Some(expected_len), reader).await {
            Ok(actual_size) => self.repository.finish_book_upload(&reservation, actual_size).await,
            Err(error) => {
                if let Err(abort_error) = self.repository.abort_book_upload(&reservation).await {
                    return Err(AssetError::Storage(format!("{error}; additionally failed to release quota reservation: {abort_error}")));
                }
                Err(error)
            }
        }
    }
}
