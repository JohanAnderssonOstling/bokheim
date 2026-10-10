//! Book access, transfer staging, and thumbnail storage.
//!
//! Native books live directly in library folders, with adjacent temporary
//! files for imports and downloads. Browser books have one hash-addressed
//! object in OPFS; their folder and trash locations exist only in SQLite.

use library_model::RelativeBookPath;
use std::fs::File;
use std::io::{ErrorKind, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use book_access::BoxedBookReader;
use library_model::BlobKind;
use std::fmt;
pub(crate) use sync_common::ContentHash;

mod import;
pub use import::ImportOptions;

#[derive(Debug)]
pub struct AssetStoreError(Arc<dyn std::error::Error + Send + Sync>);

impl AssetStoreError {
    pub fn operation(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Self(Arc::from(error.into()))
    }
}

impl fmt::Display for AssetStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for AssetStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

impl From<std::io::Error> for AssetStoreError {
    fn from(error: std::io::Error) -> Self {
        Self::operation(error)
    }
}

#[derive(Clone, Debug)]
pub struct AssetStore(imp::Handle);

impl AssetStore {
    /// Returns a sibling cover from database-selected M4B paths.
    pub async fn m4b_sidecar_cover(&self, paths: Vec<RelativeBookPath>) -> Result<Option<Vec<u8>>, AssetStoreError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let handle = self.0.clone();
            client_platform_runtime::executor::run_blocking(move || imp::m4b_sidecar_cover(&handle, &paths)).await.map_err(AssetStoreError::operation)?
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = paths;
            Ok(None)
        }
    }

    /// Opens database-selected book paths, archiving native MP3 folders for inspection.
    pub async fn open_book_reader(&self, hash: &ContentHash, paths: Vec<RelativeBookPath>) -> Result<Option<BoxedBookReader>, AssetStoreError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = hash;
            let store = self.clone();
            client_platform_runtime::executor::run_blocking(move || {
                for path in paths {
                    if let Some(reader) = imp::open_book_at_path(&store.0, &path)? {
                        return Ok(Some(reader));
                    }
                }
                Ok(None)
            })
            .await
            .map_err(AssetStoreError::operation)?
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = paths;
            imp::open_reader(&self.0, asset_name(BlobKind::Book, hash)).await
        }
    }

    pub fn book_exists(&self, hash: &ContentHash, paths: &[RelativeBookPath]) -> Result<bool, AssetStoreError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = hash;
            if !self.local_access_enabled() {
                return Ok(false);
            }
            Ok(paths.iter().any(|relative| self.placement_file_exists(relative)))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = paths;
            imp::exists(&self.0, asset_name(BlobKind::Book, hash))
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn unavailable(locator: &str) -> Self {
        Self(imp::unavailable(locator))
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn suspend_local_access(&self) {
        self.0.enabled.store(false, Ordering::Release);
    }
    pub fn local_access_enabled(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.0.enabled.load(Ordering::Acquire)
        }
        #[cfg(target_arch = "wasm32")]
        {
            true
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn local_book_bytes(&self) -> Result<u64, AssetStoreError> {
        client_platform_web::web_storage::total_file_bytes(&format!("{}/book/", self.0.namespace)).await.map_err(AssetStoreError::operation)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(library_locator: &str) -> Result<Self, AssetStoreError> {
        imp::open(library_locator).map(Self)
    }

    #[cfg(target_arch = "wasm32")]
    pub fn open(library_locator: &str, hash_service: Arc<dyn crate::hashing::HashService>) -> Result<Self, AssetStoreError> {
        imp::open(library_locator, hash_service).map(Self)
    }

    /// Opens bytes matching the requested content hash from selected native paths
    /// or browser storage, verifying the complete byte stream before upload.
    pub async fn verified_book_reader(&self, hash: ContentHash, paths: Vec<RelativeBookPath>) -> Result<Option<VerifiedBook>, AssetStoreError> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = paths;
            use std::io::{Read as _, Seek as _};
            let lease = imp::lease(&self.0, hash.to_hex().as_str(), true)?.ok_or_else(|| AssetStoreError::operation("book is in use; retry verification"))?;
            let Some(mut reader) = imp::open_verified_reader(&self.0, &hash).await? else { return Ok(None) };
            let mut hasher = self.0.hash_service.start().await.map_err(AssetStoreError::operation)?;
            let mut length = 0_u64;
            let mut buffer = vec![0_u8; 256 * 1024];
            loop {
                let read = reader.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]).await.map_err(AssetStoreError::operation)?;
                length += read as u64;
            }
            let actual: ContentHash = hasher.finish().await.map_err(AssetStoreError::operation)?;
            if actual != hash {
                return Ok(None);
            }
            reader.seek(SeekFrom::Start(0))?;
            Ok(Some(VerifiedBook { reader, length, content_hash: actual, _lease: BookLease(lease) }))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let store = self.clone();
            client_platform_runtime::executor::run_blocking(move || {
                use std::io::{Read as _, Seek as _};
                let lease = imp::lease(&store.0, hash.to_hex().as_str(), true)?.ok_or_else(|| AssetStoreError::operation("book is in use; retry verification"))?;
                for relative_path in paths {
                    let Some(mut reader): Option<BoxedBookReader> = imp::open_book_at_path(&store.0, &relative_path)? else { continue };
                    let mut hasher = blake3::Hasher::new();
                    let mut buffer = [0_u8; 64 * 1024];
                    loop {
                        let read = reader.read(&mut buffer)?;
                        if read == 0 {
                            break;
                        }
                        hasher.update(&buffer[..read]);
                    }
                    let actual = ContentHash::from(hasher.finalize());
                    if actual != hash {
                        continue;
                    }
                    reader.seek(SeekFrom::Start(0))?;
                    return Ok(Some(VerifiedBook { reader, length: hasher.count(), content_hash: actual, _lease: BookLease(lease) }));
                }
                Ok(None)
            })
            .await
            .map_err(AssetStoreError::operation)?
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn read(&self, kind: BlobKind, content_hash: &ContentHash) -> Result<Option<Vec<u8>>, AssetStoreError> {
        imp::read(&self.0, asset_name(kind, content_hash))
    }

    pub async fn read_bytes(&self, kind: BlobKind, hash: &ContentHash) -> Result<Option<Vec<u8>>, AssetStoreError> {
        self.read_named(asset_name(kind, hash)).await
    }

    pub async fn read_named(&self, name: String) -> Result<Option<Vec<u8>>, AssetStoreError> {
        #[cfg(target_arch = "wasm32")]
        {
            imp::read(&self.0, name).await
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let store = self.clone();
            client_platform_runtime::executor::run_blocking(move || imp::read(&store.0, name)).await.map_err(AssetStoreError::operation)?
        }
    }

    #[cfg(target_os = "android")]
    pub fn open_playback_file(&self, relative_path: &RelativeBookPath) -> Result<Option<File>, AssetStoreError> {
        imp::open_playback_file(&self.0, relative_path)
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn playback_reader(&self, hash: &ContentHash) -> Result<Option<client_platform_web::web_storage::FileReader>, AssetStoreError> {
        imp::open_verified_reader(&self.0, hash).await
    }

    /// Opens a native book at the selected library path.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn open_book_at_path(&self, relative_path: RelativeBookPath) -> Result<Option<BoxedBookReader>, AssetStoreError> {
        let store = self.clone();
        client_platform_runtime::executor::run_blocking(move || imp::open_book_at_path(&store.0, &relative_path)).await.map_err(AssetStoreError::operation)?
    }

    /// Opens a pinned file for independent asynchronous document cursors.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn open_async_book_at_path(&self, relative_path: RelativeBookPath) -> Result<Option<book_access::AsyncBookSource>, AssetStoreError> {
        let store = self.clone();
        let file = client_platform_runtime::executor::run_blocking(move || {
            let path = imp::placement_path(&store.0, &relative_path);
            match File::open(path) {
                Ok(file) => Ok(Some(file)),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
                Err(error) => Err(AssetStoreError::from(error)),
            }
        })
        .await
        .map_err(AssetStoreError::operation)??;
        file.map(book_access::AsyncBookSource::from_file).transpose().map_err(Into::into)
    }

    pub async fn write(&self, kind: BlobKind, content_hash: &ContentHash, bytes: &[u8]) -> Result<(), AssetStoreError> {
        imp::write(&self.0, asset_name(kind, content_hash), bytes).await?;
        if kind == BlobKind::Thumbnail {
            imp::remove(&self.0, thumbnail_variant_name(content_hash, thumbnail::BROWSE_THUMBNAIL_WIDTH))?;
        }
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn stage_book(&self, relative: RelativeBookPath, mut source: Box<dyn std::io::Read + Send>) -> Result<CopiedBook, AssetStoreError> {
        let store = self.clone();
        client_platform_runtime::executor::run_blocking(move || {
            let directory = imp::placement_path(&store.0, &relative).parent().expect("a book path has a folder").to_owned();
            let mut file = tempfile::Builder::new()
                .prefix(".bokheim-import.part-")
                .tempfile_in(&directory)
                .map_err(|error| AssetStoreError::operation(format!("could not create an import file in library folder '{}': {error}", directory.display())))?;
            imp::write_import_bytes(file.as_file_mut(), source.as_mut(), &directory)?;
            Ok(CopiedBook { inner: imp::CopiedBook { temporary_path: file.into_temp_path(), direct: None } })
        })
        .await
        .map_err(AssetStoreError::operation)?
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn begin_book_write(&self, relative: &RelativeBookPath, hash: &ContentHash) -> Result<AsyncAssetWriter, AssetStoreError> {
        let destination = imp::placement_path(&self.0, relative);
        let temporary = client_platform_runtime::executor::run_blocking(move || tempfile::Builder::new().prefix(".bokheim-download.part-").tempfile_in(destination.parent().unwrap())).await.map_err(AssetStoreError::operation)??;
        let (file, temporary_path) = temporary.into_parts();
        let inner = imp::AsyncWriter { file: tokio::fs::File::from_std(file), temporary_path };
        Ok(AsyncAssetWriter { inner, file_name: relative.as_str().to_owned(), expected_hash: *hash, hasher: blake3::Hasher::new(), store: self.clone(), written: 0, next_storage_check: 0 })
    }

    /// Copies a picked browser file into book storage while hashing it.
    #[cfg(target_arch = "wasm32")]
    pub async fn stage_browser_file(&self, file: &web_sys::File) -> Result<StagedBook, AssetStoreError> {
        Ok(StagedBook { inner: imp::stage_browser_file(&self.0, file).await? })
    }

    #[cfg(target_arch = "wasm32")]
    pub fn finish_staged_write(&self, writer: client_platform_web::web_storage::FileWriter, content_hash: ContentHash) -> Result<StagedBook, AssetStoreError> {
        Ok(StagedBook { inner: imp::finish_staged_write(&self.0, writer, content_hash)? })
    }

    /// Capacity is an adapter capability; unknown capacity relies on I/O quota errors.
    pub fn check_write_capacity(&self, additional_bytes: u64) -> Result<(), AssetStoreError> {
        if let Some(available) = imp::write_capacity(&self.0)? {
            if additional_bytes > available {
                return Err(AssetStoreError::operation("not enough free storage to write book".to_owned()));
            }
        }
        Ok(())
    }

    pub async fn write_thumbnail_variant(&self, content_hash: &ContentHash, width: u32, bytes: &[u8]) -> Result<(), AssetStoreError> {
        imp::write(&self.0, thumbnail_variant_name(content_hash, width), bytes).await
    }

    /// Hold one exclusive lease across both variants, including blocking writes
    /// after cancellation, so an upload cannot read mixed generations.
    pub async fn write_thumbnail_set(&self, hash: &ContentHash, bytes: &[u8], browse: &[u8]) -> Result<(), AssetStoreError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            imp::write_thumbnail_set(&self.0, hash, bytes, browse).await
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _lease = self.try_exclusive_lease(hash)?.ok_or_else(|| AssetStoreError::operation("thumbnail is being updated"))?;
            self.write(BlobKind::Thumbnail, hash, bytes).await?;
            self.write_thumbnail_variant(hash, thumbnail::BROWSE_THUMBNAIL_WIDTH, browse).await
        }
    }

    pub fn thumbnail_set_exists(&self, content_hash: &ContentHash) -> Result<bool, AssetStoreError> {
        Ok(self.exists(BlobKind::Thumbnail, content_hash)? && imp::exists(&self.0, thumbnail_variant_name(content_hash, thumbnail::BROWSE_THUMBNAIL_WIDTH))?)
    }

    pub fn remove(&self, kind: BlobKind, content_hash: &ContentHash) -> Result<bool, AssetStoreError> {
        let removed = imp::remove(&self.0, asset_name(kind, content_hash))?;
        if kind == BlobKind::Thumbnail {
            imp::remove(&self.0, thumbnail_variant_name(content_hash, thumbnail::BROWSE_THUMBNAIL_WIDTH))?;
        }
        Ok(removed)
    }

    pub fn exists(&self, kind: BlobKind, content_hash: &ContentHash) -> Result<bool, AssetStoreError> {
        imp::exists(&self.0, asset_name(kind, content_hash))
    }

    /// Whether a library-relative placement path is a readable file. Lets the
    /// import orchestrator resolve materialization work from a database snapshot
    /// without holding the writer.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn placement_file_exists(&self, relative: &RelativeBookPath) -> bool {
        let path = imp::placement_path(&self.0, relative);
        path.is_file() || path.is_dir() && audiobook_folder::discover(&path).is_ok_and(|tracks| !tracks.is_empty())
    }

    /// Best-effort filesystem ensure of a placement's parent directory, with
    /// no database involvement. Directory identity healing stays with the
    /// directory passes; this only keeps a download from failing on a
    /// missing folder that its snapshot still considers live.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn ensure_placement_parent(&self, relative: &RelativeBookPath) -> Result<(), AssetStoreError> {
        std::fs::create_dir_all(imp::placement_path(&self.0, relative).parent().expect("a book path has a folder"))?;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn evict_book_files(&self, relative_paths: &[RelativeBookPath], hash: &ContentHash) -> Result<(), AssetStoreError> {
        imp::evict_book_files(&self.0, relative_paths, hash)
    }

    /// Try to hold a book exclusively while publishing or removing bytes.
    pub fn try_exclusive_lease(&self, hash: &ContentHash) -> Result<Option<BookLease>, AssetStoreError> {
        imp::lease(&self.0, hash.to_hex().as_str(), true).map(|lease| lease.map(BookLease))
    }

    /// Hold across book and the referencing metadata commit. Cleanup
    /// skips leased hashes, including leases held by another native process.
    pub async fn lease_book(&self, hash: &ContentHash) -> Result<BookLease, AssetStoreError> {
        loop {
            if let Some(lease) = imp::lease(&self.0, hash.to_hex().as_str(), false)? {
                return Ok(BookLease(lease));
            }
            client_platform_runtime::executor::sleep(Duration::from_millis(25)).await;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn lease_book_blocking(&self, hash: &ContentHash) -> Result<BookLease, AssetStoreError> {
        loop {
            if let Some(lease) = imp::lease(&self.0, hash.to_hex().as_str(), false)? {
                return Ok(BookLease(lease));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

impl AssetStore {
    /// Count already-resolved native book paths. Resolving placements is
    /// database policy and deliberately belongs to the caller.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn book_bytes_at_paths(&self, relative_paths: Vec<RelativeBookPath>) -> Result<u64, AssetStoreError> {
        let store = self.clone();
        client_platform_runtime::executor::run_blocking(move || {
            let mut total = 0_u64;
            let mut counted = std::collections::HashSet::new();
            for relative in relative_paths {
                let path = imp::placement_path(&store.0, &relative);
                if counted.insert(path.clone()) {
                    match std::fs::metadata(path) {
                        Ok(metadata) if metadata.is_file() => total = total.saturating_add(metadata.len()),
                        Ok(_) => {}
                        Err(error) if error.kind() == ErrorKind::NotFound => {}
                        Err(error) => return Err(error.into()),
                    }
                }
            }
            Ok(total)
        })
        .await
        .map_err(AssetStoreError::operation)?
    }
}

pub struct VerifiedBook {
    #[cfg(not(target_arch = "wasm32"))]
    reader: BoxedBookReader,
    #[cfg(target_arch = "wasm32")]
    reader: client_platform_web::web_storage::FileReader,
    pub length: u64,
    pub content_hash: ContentHash,
    _lease: BookLease,
}

impl VerifiedBook {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn into_upload_stream(self) -> impl futures_util::Stream<Item = Result<Vec<u8>, std::io::Error>> + Send {
        imp::upload_stream(self)
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn upload_file(&self) -> Result<web_sys::Blob, AssetStoreError> {
        self.reader.upload_file().await.map_err(AssetStoreError::operation)
    }
}

pub struct BookLease(#[allow(dead_code)] imp::Lease);

/// A downloaded book being written to disk, hashed as it arrives.
#[cfg(not(target_arch = "wasm32"))]
pub struct AsyncAssetWriter {
    file_name: String,
    inner: imp::AsyncWriter,
    expected_hash: ContentHash,
    hasher: blake3::Hasher,
    store: AssetStore,
    written: u64,
    next_storage_check: u64,
}

#[cfg(not(target_arch = "wasm32"))]
impl AsyncAssetWriter {
    pub async fn write_all(&mut self, buffer: &[u8]) -> Result<(), AssetStoreError> {
        if self.written >= self.next_storage_check {
            self.store.check_write_capacity(buffer.len() as u64)?;
            self.next_storage_check = self.written.saturating_add(8 * 1024 * 1024);
        }
        self.inner.write_all(buffer).await?;
        self.hasher.update(buffer);
        self.written = self.written.checked_add(buffer.len() as u64).ok_or_else(|| AssetStoreError::operation("asset length overflow"))?;
        Ok(())
    }

    pub async fn verify(self) -> Result<VerifiedAssetWriter, AssetStoreError> {
        let actual = ContentHash::from(self.hasher.finalize());
        if actual != self.expected_hash {
            log::warn!("file verification failed for {:?}: expected={} got={actual}", self.file_name, self.expected_hash);
            return Err(AssetStoreError::operation("file verification failed"));
        }
        Ok(VerifiedAssetWriter(self))
    }
}

/// Verified bytes that can be finalized but can no longer be written.
#[cfg(not(target_arch = "wasm32"))]
pub struct VerifiedAssetWriter(AsyncAssetWriter);

#[cfg(not(target_arch = "wasm32"))]
impl VerifiedAssetWriter {
    pub async fn finish_book(self) -> Result<StagedBook, AssetStoreError> {
        // Byte verification already matched the requested content hash.
        let temporary_path = self.0.inner.finish_book().await?;
        Ok(StagedBook { inner: imp::StagedBook { temporary_path, content_hash: self.0.expected_hash, size_bytes: self.0.written, direct: None } })
    }
}

pub struct StagedBook {
    inner: imp::StagedBook,
}

/// Copied native bytes awaiting the shared file-ingestion workflow.
#[cfg(not(target_arch = "wasm32"))]
pub struct CopiedBook {
    inner: imp::CopiedBook,
}

#[cfg(target_arch = "wasm32")]
pub type CopiedBook = StagedBook;

#[cfg(not(target_arch = "wasm32"))]
impl CopiedBook {
    pub fn path(&self) -> &Path {
        &self.inner.temporary_path
    }

    pub fn open_reader(&self) -> Result<BoxedBookReader, AssetStoreError> {
        Ok(Box::new(File::open(self.path())?))
    }

    pub fn into_staged(self, content_hash: ContentHash, size_bytes: u64) -> StagedBook {
        StagedBook { inner: imp::StagedBook { temporary_path: self.inner.temporary_path, direct: self.inner.direct, content_hash, size_bytes } }
    }
}

pub use library_model::PublishedStagedBook;

impl StagedBook {
    /// Publishes bytes from an immutable database snapshot. The caller records
    /// the resulting placement through the database acknowledgement API.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn publish_snapshot(self, store: &AssetStore, relative_path: &RelativeBookPath, occupied_names: &[String], audiobook_archive: bool) -> Result<PublishedStagedBook, AssetStoreError> {
        let hash = self.content_hash();
        let imp::StagedBook { temporary_path, direct, .. } = self.inner;
        // A direct import already wrote its bytes straight to the final
        // destination; re-running the move/dedup logic below would find that
        // path already occupied by itself and discard it as a duplicate.
        if let Some(direct) = direct {
            imp::sync_parent(&direct.path)?;
            let fingerprint = Some(client_platform_native::file_fingerprint::file_metadata_fingerprint(&std::fs::metadata(&direct.path)?)?.to_string());
            let name = relative_path.file_name().to_owned();
            temporary_path.keep().map_err(|error| AssetStoreError::operation(format!("could not finalize the imported book '{}': {}", direct.path.display(), error.error)))?;
            return Ok(PublishedStagedBook { name, relative_path: relative_path.to_string(), published: true, fingerprint });
        }
        // The caller knows the book's format; an MP3-folder book travels as a
        // ZIP of its tracks and is published as a folder.
        if audiobook_archive {
            let requested = relative_path.file_name();
            let mut alternatives = library_replica::folder_name_candidates(requested, occupied_names.iter().map(String::as_str).chain(std::iter::once(requested)));
            let mut name = requested.to_owned();
            loop {
                let relative = relative_path.with_file_name(&name).map_err(AssetStoreError::operation)?;
                let destination = imp::placement_path(&store.0, &relative);
                if destination.exists() {
                    if destination.is_dir() {
                        let tracks = audiobook_folder::discover(&destination)?;
                        if !tracks.is_empty() && ContentHash::from(audiobook_folder::folder_hash(&destination)?) == hash {
                            temporary_path.close()?;
                            return Ok(PublishedStagedBook { name, relative_path: relative.to_string(), published: false, fingerprint: None });
                        }
                    }
                    name = alternatives.next().expect("numeric name suffix space is unbounded");
                    continue;
                }
                let staged = tempfile::Builder::new().prefix(".bokheim-audio-").tempdir_in(destination.parent().unwrap())?;
                audiobook_archive::extract_archive(File::open(&temporary_path)?, staged.path())?;
                let tracks = audiobook_folder::discover(staged.path())?;
                let fingerprint = crate::filesystem::audiobook_fingerprint(staged.path(), &tracks)?.to_string();
                std::fs::rename(staged.path(), &destination)?;
                temporary_path.close()?;
                imp::sync_parent(&destination)?;
                return Ok(PublishedStagedBook { name, relative_path: relative.to_string(), published: true, fingerprint: Some(fingerprint) });
            }
        }
        let name = relative_path.file_name();
        let mut alternatives = library_replica::file_name_candidates(name, occupied_names.iter().map(String::as_str).chain(std::iter::once(name)));
        let mut chosen = name.to_owned();
        let mut temporary = temporary_path;
        loop {
            let relative = relative_path.with_file_name(&chosen).map_err(AssetStoreError::operation)?;
            if !imp::placement_conflicts(&store.0, &hash, &relative)? {
                let destination = imp::placement_path(&store.0, &relative);
                let published = if destination.is_file() {
                    temporary.close()?;
                    false
                } else {
                    match imp::persist_book_noclobber(temporary, &destination) {
                        Ok(()) => true,
                        Err(error) if error.error.kind() == ErrorKind::AlreadyExists => {
                            temporary = error.path;
                            chosen = alternatives.next().expect("numeric name suffix space is unbounded");
                            continue;
                        }
                        Err(error) => return Err(AssetStoreError::operation(format!("could not move the temporary book to '{}' without replacing existing files: {}", destination.display(), error.error))),
                    }
                };
                imp::sync_parent(&destination)?;
                let fingerprint = if published { Some(client_platform_native::file_fingerprint::file_metadata_fingerprint(&std::fs::metadata(&destination)?)?.to_string()) } else { None };
                return Ok(PublishedStagedBook { name: chosen, relative_path: relative.to_string(), published, fingerprint });
            }
            chosen = alternatives.next().expect("numeric name suffix space is unbounded");
        }
    }

    pub fn content_hash(&self) -> ContentHash {
        self.inner.content_hash
    }
    pub fn size_bytes(&self) -> u64 {
        self.inner.size_bytes
    }
    #[cfg(target_arch = "wasm32")]
    pub fn open_reader(&self) -> Result<BoxedBookReader, AssetStoreError> {
        Ok(Box::new(self.inner.writer.open_reader()))
    }
    #[cfg(target_arch = "wasm32")]
    pub async fn commit(self) -> Result<(), AssetStoreError> {
        self.inner.commit().await
    }
}

fn asset_name(kind: BlobKind, content_hash: &ContentHash) -> String {
    format!("{}/{content_hash}", kind.storage())
}

pub fn thumbnail_variant_name(content_hash: &ContentHash, width: u32) -> String {
    format!("thumbnail/{content_hash}.{width}w")
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native as imp;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn path(value: &str) -> RelativeBookPath {
        RelativeBookPath::parse(value).unwrap()
    }

    #[tokio::test]
    async fn mp3_folder_download_extracts_and_upload_repackages_same_revision() {
        let library = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        std::fs::create_dir(source.path().join("Disc 1")).unwrap();
        std::fs::write(source.path().join("Disc 1/01.mp3"), b"opening").unwrap();
        std::fs::write(source.path().join("Disc 1/02.mp3"), b"ending").unwrap();
        std::fs::write(source.path().join("book.nfo"), b"Title: Story\n").unwrap();
        std::fs::write(source.path().join("book.cue"), b"FILE \"Disc 1/01.mp3\" MP3\nTRACK 01 AUDIO\nTITLE \"Opening\"\nINDEX 01 00:00:00\n").unwrap();
        std::fs::write(source.path().join("cover.jpg"), b"cover fixture").unwrap();
        let tracks = audiobook_folder::discover(source.path()).unwrap();
        let hash = ContentHash::from(audiobook_folder::folder_hash(source.path()).unwrap());
        let nfo = audiobook_folder::discover_nfo(source.path()).unwrap();
        let cue = audiobook_folder::discover_cue(source.path()).unwrap();
        let cover = audiobook_folder::discover_cover(source.path()).unwrap();
        let archive = audiobook_archive::write_archive_with_sidecars(std::io::Cursor::new(Vec::new()), &tracks, nfo.as_ref(), cue.as_ref(), cover.as_ref()).unwrap().into_inner();
        assert_eq!(hash, ContentHash::from(blake3::hash(&archive)));
        let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let mut writer = store.begin_book_write(&path("Story"), &hash).await.unwrap();
        writer.write_all(&archive).await.unwrap();
        let staged = writer.verify().await.unwrap().finish_book().await.unwrap();
        let published = staged.publish_snapshot(&store, &path("Story"), &[], true).unwrap();
        assert!(published.published);
        assert_eq!(published.name, "Story");
        assert_eq!(std::fs::read(library.path().join("Story/Disc 1/02.mp3")).unwrap(), b"ending");
        assert_eq!(std::fs::read(library.path().join("Story/book.nfo")).unwrap(), b"Title: Story\n");
        assert_eq!(std::fs::read(library.path().join("Story/book.cue")).unwrap(), b"FILE \"Disc 1/01.mp3\" MP3\nTRACK 01 AUDIO\nTITLE \"Opening\"\nINDEX 01 00:00:00\n");
        assert_eq!(std::fs::read(library.path().join("Story/cover.jpg")).unwrap(), b"cover fixture");
        assert!(store.book_exists(&hash, &[path("Story")]).unwrap());
        let verified = store.verified_book_reader(hash, vec![path("Story")]).await.unwrap().unwrap();
        assert_eq!(verified.length, archive.len() as u64);
        assert_eq!(verified.content_hash, hash);
        use futures_util::StreamExt as _;
        let result = verified.into_upload_stream().collect::<Vec<_>>().await.into_iter().collect::<Result<Vec<_>, _>>().unwrap().concat();
        assert_eq!(result, archive);
    }

    #[tokio::test]
    async fn download_rejects_bytes_that_do_not_match_the_book_hash() {
        let library = tempfile::tempdir().unwrap();
        let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let hash = ContentHash::from(blake3::hash(b"expected book"));
        let mut writer = store.begin_book_write(&path("book.epub"), &hash).await.unwrap();
        writer.write_all(b"different book").await.unwrap();
        assert!(writer.verify().await.is_err());
        assert!(!library.path().join("book.epub").exists());
    }

    #[tokio::test]
    async fn failed_thumbnail_book_preserves_the_browse_variant() {
        let library = tempfile::tempdir().unwrap();
        let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let hash = ContentHash::from(blake3::hash(b"thumbnail owner"));
        store.write_thumbnail_variant(&hash, thumbnail::BROWSE_THUMBNAIL_WIDTH, b"previous browse cover").await.unwrap();
        let destination = library.path().join(crate::APP_HIDDEN_DIR).join("assets/thumbnail").join(hash.to_hex().as_str());
        std::fs::create_dir(&destination).unwrap();
        assert!(store.write(BlobKind::Thumbnail, &hash, b"replacement cover").await.is_err());
        assert_eq!(store.read_named(thumbnail_variant_name(&hash, thumbnail::BROWSE_THUMBNAIL_WIDTH)).await.unwrap().unwrap(), b"previous browse cover");
        assert_eq!(std::fs::read_dir(destination.parent().unwrap()).unwrap().count(), 2, "failed book must remove its temporary file");
    }

    #[tokio::test]
    async fn abandoned_and_invalid_downloads_remove_adjacent_temporary_files() {
        let library = tempfile::tempdir().unwrap();
        let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let hash = ContentHash::from(blake3::hash(b"complete"));
        std::fs::write(library.path().join("book.epub"), b"existing user file").unwrap();
        for verify in [false, true] {
            let mut writer = store.begin_book_write(&path("/book.epub"), &hash).await.unwrap();
            writer.write_all(b"incomplete").await.unwrap();
            if verify {
                assert!(writer.verify().await.is_err());
            } else {
                drop(writer);
            }
            assert!(!std::fs::read_dir(library.path()).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().contains(".part-")));
            assert_eq!(std::fs::read(library.path().join("book.epub")).unwrap(), b"existing user file");
        }
        assert!(!library.path().join(".bokheim/assets/book").exists());
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) mod browser;
#[cfg(target_arch = "wasm32")]
pub(crate) use browser as imp;

#[cfg(not(target_arch = "wasm32"))]
pub fn import_in_progress(path: &Path) -> bool {
    imp::import_in_progress(path)
}

#[cfg(not(target_arch = "wasm32"))]
mod native_placement;
#[cfg(not(target_arch = "wasm32"))]
pub use native_placement::{DirectoryPlacement, PreparedDirectory, PreparedPlacement};
