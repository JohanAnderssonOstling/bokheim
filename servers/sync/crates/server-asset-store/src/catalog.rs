//! Public catalog adoption. A private hard link pins the verified inode on
//! the catalog filesystem; neither sync publication nor cleanup writes to the
//! public catalog tree. No copy fallback is permitted across filesystems.

use crate::{remove_if_present, storage_error, AssetError, AssetKind, FsAssetStore, TemporaryFile, MAX_BOOK_UPLOAD_DURATION};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use sync_common::BookId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogStorage {
    /// Only publicly downloadable, immutable `<BLAKE3>.epub` files belong here.
    pub books: PathBuf,
    /// Private directory on the same filesystem, outside the public web root.
    pub pins: PathBuf,
}

/// A verified hard link whose temporary name protects it from collection while
/// the caller awaits database authorization. Dropping it releases the link.
pub struct PreparedCatalogBook {
    temp: TemporaryFile,
    pin: PathBuf,
    duplicate: PathBuf,
    size: u64,
}

fn temporary_pin(pins: &Path) -> PathBuf {
    let created = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    pins.join(format!(".prepare-{created}-{}", uuid::Uuid::new_v4()))
}

impl PreparedCatalogBook {
    pub fn size_bytes(&self) -> u64 {
        self.size
    }

    /// Caller must hold the exclusive asset-publication database lock. This is
    /// synchronous so cancellation cannot publish or unlink after lock release.
    pub fn publish(self) -> Result<(), AssetError> {
        // Atomic replacement also handles a publisher installing a new inode
        // with identical bytes at an existing public content-hash URL.
        std::fs::rename(self.temp.path(), &self.pin).map_err(storage_error)?;
        std::fs::File::open(self.pin.parent().expect("catalog pin has a parent")).and_then(|directory| directory.sync_all()).map_err(storage_error)?;
        // Consolidate a previously uploaded copy only after the retained link
        // is durable. Existing open readers retain their original file handle.
        remove_if_present(&self.duplicate)?;
        self.temp.remove().map_err(storage_error)
    }
}

impl FsAssetStore {
    pub fn with_catalog(mut self, catalog: CatalogStorage) -> Self {
        self.catalog = Some(catalog);
        self
    }

    pub(super) fn catalog_pin(&self, hash: &BookId) -> Option<PathBuf> {
        self.catalog.as_ref().map(|catalog| catalog.pins.join(hash.to_hex().as_str()))
    }

    /// Only a currently public catalog path can authorize a new account. A
    /// retained link alone is storage for existing owners, not public evidence.
    pub async fn prepare_catalog_book(&self, hash: &BookId) -> Result<Option<PreparedCatalogBook>, AssetError> {
        let Some(catalog) = &self.catalog else { return Ok(None) };

        let source = catalog.books.join(format!("{hash}.epub"));
        let pin = catalog.pins.join(hash.to_hex().as_str());
        let duplicate = self.path("", AssetKind::Book, hash)?;
        let temporary = temporary_pin(&catalog.pins);
        let hash = *hash;
        tokio::task::spawn_blocking(move || {
            match std::fs::symlink_metadata(&source) {
                Ok(metadata) if metadata.is_file() => {}
                Ok(_) => return Err(AssetError::InvalidInput("public catalog book is not a regular file".into())),
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(storage_error(error)),
            }
            let temp = TemporaryFile::new(temporary);
            match std::fs::hard_link(&source, temp.path()) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(AssetError::Storage(format!("cannot retain public catalog book: {error}; catalog books and pins must allow hard links on one filesystem"))),
            }
            // Recheck after linking so a replaced symlink cannot authorize a
            // private file outside the trusted public directory.
            if !std::fs::symlink_metadata(temp.path()).map_err(storage_error)?.is_file() {
                return Err(AssetError::InvalidInput("public catalog book is not a regular file".into()));
            }
            let mut file = std::fs::File::open(temp.path()).map_err(storage_error)?;
            let size = file.metadata().map_err(storage_error)?.len();
            let digest = blake3::Hasher::new().update_reader(&mut file).map_err(storage_error)?.finalize();
            if digest.as_bytes() != hash.as_bytes() {
                return Err(AssetError::HashMismatch);
            }
            Ok(Some(PreparedCatalogBook { temp, pin, duplicate, size }))
        })
        .await
        .map_err(|error| AssetError::Storage(error.to_string()))?
    }

    /// Background consolidation of uploads that predate catalog reuse. Avoid
    /// rehashing books whose retained and public paths already share an inode.
    pub async fn prepare_catalog_duplicate(&self, hash: &BookId) -> Result<Option<PreparedCatalogBook>, AssetError> {
        let Some(catalog) = &self.catalog else { return Ok(None) };
        match tokio::fs::symlink_metadata(self.path("", AssetKind::Book, hash)?).await {
            Ok(metadata) if metadata.is_file() => return self.prepare_catalog_book(hash).await,
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(storage_error(error)),
        }
        let pin = catalog.pins.join(hash.to_hex());
        let retained = match tokio::fs::symlink_metadata(pin).await {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(storage_error(error)),
        };
        let public = match tokio::fs::symlink_metadata(catalog.books.join(format!("{hash}.epub"))).await {
            Ok(metadata) if metadata.is_file() => metadata,
            Ok(_) => return Ok(None),
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(storage_error(error)),
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if retained.dev() == public.dev() && retained.ino() == public.ino() {
                return Ok(None);
            }
        }
        #[cfg(not(unix))]
        let _ = (retained, public);
        self.prepare_catalog_book(hash).await
    }

    /// Recover temporary links left by process crashes. Link mtimes belong to
    /// the catalog inode and cannot measure preparation age, hence the explicit
    /// creation time in temporary names. Published pins have only a hash name.
    pub async fn remove_stale_catalog_preparations(&self) -> Result<(), AssetError> {
        let Some(catalog) = &self.catalog else { return Ok(()) };
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
        let grace = MAX_BOOK_UPLOAD_DURATION.as_secs() + 5 * 60;
        let mut entries = tokio::fs::read_dir(&catalog.pins).await.map_err(storage_error)?;
        while let Some(entry) = entries.next_entry().await.map_err(storage_error)? {
            let name = entry.file_name();
            let Some((created, id)) = name.to_str().and_then(|name| name.strip_prefix(".prepare-")).and_then(|name| name.split_once('-')) else {
                continue;
            };
            let Ok(created) = created.parse::<u64>() else {
                continue;
            };
            if uuid::Uuid::parse_str(id).is_ok() && now.saturating_sub(created) > grace && entry.file_type().await.map_err(storage_error)?.is_file() {
                remove_if_present(&entry.path())?;
            }
        }
        Ok(())
    }

    pub(super) async fn initialize_catalog(&self, create: bool) -> Result<(), AssetError> {
        let Some(catalog) = &self.catalog else { return Ok(()) };
        let books = tokio::fs::canonicalize(&catalog.books).await.map_err(storage_error)?;
        if !tokio::fs::metadata(&books).await.map_err(storage_error)?.is_dir() {
            return Err(AssetError::Storage("public catalog books path is not a directory".into()));
        }
        Self::probe_root("catalog pin", &catalog.pins, create).await?;
        let pins = tokio::fs::canonicalize(&catalog.pins).await.map_err(storage_error)?;
        if pins.starts_with(&books) || books.starts_with(&pins) {
            return Err(AssetError::Storage("catalog books and private pins must be separate directories".into()));
        }
        if books.parent().is_some_and(|public| pins.starts_with(public)) {
            return Err(AssetError::Storage("private catalog pins must be outside the public catalog directory".into()));
        }
        for root in [&self.books_root, &self.thumbnails_root] {
            let root = tokio::fs::canonicalize(root).await.map_err(storage_error)?;
            if pins.starts_with(&root) || root.starts_with(&pins) || books.starts_with(&root) || root.starts_with(&books) {
                return Err(AssetError::Storage("catalog storage must be separate from private upload and thumbnail directories".into()));
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if tokio::fs::metadata(&books).await.map_err(storage_error)?.dev() != tokio::fs::metadata(&pins).await.map_err(storage_error)?.dev() {
                return Err(AssetError::Storage("catalog books and private pins must be on the same filesystem".into()));
            }
        }
        if create {
            let mut entries = tokio::fs::read_dir(&books).await.map_err(storage_error)?;
            while let Some(entry) = entries.next_entry().await.map_err(storage_error)? {
                let name = entry.file_name();
                let hosted = name.to_str().and_then(|name| name.strip_suffix(".epub")).and_then(|hash| hash.parse::<BookId>().ok());
                if hosted.is_none() || !entry.file_type().await.map_err(storage_error)?.is_file() {
                    continue;
                }
                let source = entry.path();
                let temporary = temporary_pin(&pins);
                tokio::task::spawn_blocking(move || {
                    let probe = TemporaryFile::new(temporary);
                    std::fs::hard_link(&source, probe.path()).map_err(|error| AssetError::Storage(format!("catalog hard-link probe failed: {error}; check filesystem, service mount namespace, and hard-link permissions")))?;
                    probe.remove().map_err(storage_error)
                })
                .await
                .map_err(|error| AssetError::Storage(error.to_string()))??;
                break;
            }
        }
        Ok(())
    }

    pub(super) async fn resolved_path(&self, user: &str, kind: AssetKind, hash: &BookId) -> Result<PathBuf, AssetError> {
        if kind == AssetKind::Book {
            if let Some(pin) = self.catalog_pin(hash) {
                match tokio::fs::symlink_metadata(&pin).await {
                    Ok(metadata) if metadata.is_file() => return Ok(pin),
                    Ok(_) => return Err(AssetError::Storage("retained catalog book is not a regular file".into())),
                    Err(error) if error.kind() == ErrorKind::NotFound => {}
                    Err(error) => return Err(storage_error(error)),
                }
            }
        }
        self.path(user, kind, hash)
    }
}

#[cfg(test)]
mod tests;
