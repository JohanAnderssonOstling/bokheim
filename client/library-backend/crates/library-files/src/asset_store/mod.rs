//! Book access, transfer staging, and thumbnail storage.
//!
//! Native books live directly in library folders, with adjacent temporary
//! files for imports and downloads. Browser books have one hash-addressed
//! object in OPFS; their folder and trash locations exist only in SQLite.

use book_access::BoxedBookReader;
#[cfg(not(target_arch = "wasm32"))]
use client_platform_native::file_fingerprint::file_fingerprint;
#[cfg(not(target_arch = "wasm32"))]
use client_platform_native::filesystem::parent_directory;
use client_platform_runtime::executor::run_blocking;
#[cfg(target_arch = "wasm32")]
use client_platform_runtime::executor::storage_call;
#[cfg(target_arch = "wasm32")]
use client_platform_web::web_storage::PublishedFile;
#[cfg(not(target_arch = "wasm32"))]
use content_address::ContentHash;
use library_model::BlobKind;
use library_model::RelativeBookPath;
use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::fs::File;
#[cfg(not(target_arch = "wasm32"))]
use std::io::{Error, ErrorKind};
use std::io::{Read, Seek, SeekFrom};
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::Ordering;

use std::time::Duration;
pub(crate) use sync_common::BookId;
#[cfg(not(target_arch = "wasm32"))]
use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard};

#[cfg(target_arch = "wasm32")]
mod browser_transfer;
mod import;
mod text_cache;
pub use text_cache::TextCacheWriter;
#[cfg(not(target_arch = "wasm32"))]
mod import_batch;
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
    /// Opening runs this before actors or readers can hold old identities.
    pub async fn migrate_book_ids(&self, ids: Vec<(BookId, BookId)>) -> Result<(), AssetStoreError> {
        let store = self.clone();
        run_blocking(move || imp::migrate_book_ids(&store.0, &ids)).await.map_err(AssetStoreError::operation)?
    }

    /// Short native file operations run inline. Browser metadata includes the
    /// OPFS index, so it stays on the storage worker with its SQLite connection.
    async fn metadata<T: Send + 'static>(&self, operation: impl FnOnce(&Self) -> Result<T, AssetStoreError> + Send + 'static) -> Result<T, AssetStoreError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            operation(self)
        }
        #[cfg(target_arch = "wasm32")]
        {
            let store = self.clone();
            run_blocking(move || operation(&store)).await.map_err(AssetStoreError::operation)?
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn native_library_root(&self) -> &Path {
        &self.0.library_root
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn native_storage_identity(&self) -> &str {
        &self.0.storage_identity
    }

    /// Invalidate clones retained by cancelled work when switching roots.
    /// The caller holds exclusive root access after draining foreground work.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn retire(&self) {
        self.0.enabled.store(false, Ordering::Release);
        self.0.root_failure.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
    }

    /// Keep the root in place through independent preparation and cleanup.
    /// Acquire before scheduling blocking work so queued calls are covered too.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn lock_root_shared(&self) -> OwnedRwLockReadGuard<()> {
        self.0.root_access.clone().read_owned().await
    }

    /// Stop workers before waiting: their blocking calls retain shared access
    /// until they have finished or cleaned up their owned files.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn lock_root_exclusive(&self) -> OwnedRwLockWriteGuard<()> {
        self.0.root_access.clone().write_owned().await
    }

    /// Returns a sibling cover from database-selected M4B paths.
    pub async fn m4b_sidecar_cover(&self, paths: Vec<RelativeBookPath>) -> Result<Option<Vec<u8>>, AssetStoreError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let handle = self.0.clone();
            run_blocking(move || imp::m4b_sidecar_cover(&handle, &paths)).await.map_err(AssetStoreError::operation)?
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = paths;
            Ok(None)
        }
    }

    /// Opens database-selected book paths, archiving native MP3 folders for inspection.
    pub async fn open_book_reader(&self, hash: &BookId, paths: Vec<RelativeBookPath>) -> Result<Option<BoxedBookReader>, AssetStoreError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = hash;
            let store = self.clone();
            run_blocking(move || {
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

    #[cfg(not(target_arch = "wasm32"))]
    pub fn remember_local_content(&self, book: BookId, checksum: ContentHash) -> Result<(), AssetStoreError> {
        imp::remember_local_checksum(&self.0, &book, checksum)
    }

    /// Persist one scan batch's checksum proofs without holding library state.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn remember_local_contents(&self, contents: Vec<(BookId, ContentHash)>) -> Result<(), AssetStoreError> {
        let store = self.for_import_batch();
        let root = self.lock_root_shared().await;
        run_blocking(move || {
            let _root = root;
            if !store.root_available() {
                return Err(AssetStoreError::operation("library folder is unavailable"));
            }
            for (book, checksum) in contents {
                store.remember_local_content(book, checksum)?;
            }
            // One durability boundary for the scan batch. Proofs must survive
            // before the caller commits the identities that depend on them.
            store.flush_import_batch_blocking()?;
            if !store.root_available() {
                return Err(AssetStoreError::operation("library folder changed during scan"));
            }
            Ok(())
        })
        .await
        .map_err(AssetStoreError::operation)?
    }

    /// Native directory placements require audiobook discovery, including track
    /// tags. Keep that potentially long inspection on the blocking worker.
    pub async fn book_exists(&self, hash: &BookId, paths: &[RelativeBookPath]) -> Result<bool, AssetStoreError> {
        let store = self.clone();
        let owned_hash = *hash;
        let owned_paths = paths.to_vec();
        #[cfg(not(target_arch = "wasm32"))]
        let root = self.lock_root_shared().await;
        run_blocking(move || {
            #[cfg(not(target_arch = "wasm32"))]
            let _root = root;
            let hash = &owned_hash;
            let paths = owned_paths.as_slice();
            #[cfg(not(target_arch = "wasm32"))]
            {
                let _ = hash;
                if store.is_retired() {
                    return Ok(false);
                }
                let present = paths.iter().any(|relative| store.placement_file_exists(relative));
                // Mount replacement can make the same relative path readable on
                // another storage instance. Validate it as part of this access.
                Ok(store.root_available() && present)
            }
            #[cfg(target_arch = "wasm32")]
            {
                let _ = paths;
                imp::exists(&store.0, asset_name(BlobKind::Book, hash))
            }
        })
        .await
        .map_err(AssetStoreError::operation)?
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn is_retired(&self) -> bool {
        !self.0.enabled.load(Ordering::Acquire)
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

    pub async fn verified_book_reader_with_checksum(&self, hash: BookId, paths: Vec<RelativeBookPath>, checksum: Option<content_address::ContentHash>) -> Result<Option<VerifiedBook>, AssetStoreError> {
        // Keep publication and deletion out while checking the bytes, while
        // allowing other readers. Wait outside the blocking pool so a
        // concurrent import can finish and release its publication lease.
        let lease = self.lease_book(&hash).await?;
        #[cfg(target_arch = "wasm32")]
        {
            let _ = paths;
            let Some(reader) = imp::open_verified_reader(&self.0, &hash).await? else { return Ok(None) };
            let (actual, length) = reader.checksum().await.map_err(AssetStoreError::operation)?;
            let actual = BookId::from(actual);
            if actual.content_hash() != checksum.or_else(|| hash.content_hash()) {
                return Ok(None);
            }
            Ok(Some(VerifiedBook { reader, length, book_id: actual, _lease: lease }))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let store = self.clone();
            run_blocking(move || {
                use std::io::Seek as _;
                for relative_path in paths {
                    let expected = checksum.or(hash.content_hash());
                    let path = imp::placement_path(&store.0, &relative_path);
                    let folder_revision = if path.is_dir() { Some(crate::filesystem::audiobook_fingerprint(&path, &audiobook_folder::discover(&path)?)?) } else { None };
                    // A newly scanned audiobook has a recording identity but
                    // no ZIP-byte checksum yet. Verify its member proof before
                    // constructing the transport, and pin that source revision.
                    let source = if expected.is_none() {
                        if folder_revision.is_none() {
                            continue;
                        }
                        let Some(source) = imp::VerifiedPath::matching(path.clone(), &hash)? else { continue };
                        Some(source)
                    } else {
                        None
                    };
                    let Some(mut reader): Option<BoxedBookReader> = imp::open_book_at_path(&store.0, &relative_path)? else { continue };
                    let mut hasher = blake3::Hasher::new();
                    hasher.update_reader(&mut reader)?;
                    let actual = BookId::from(hasher.finalize());
                    if expected.is_some() && actual.content_hash() != expected {
                        continue;
                    }
                    if let Some(source) = source {
                        source.check_metadata()?;
                    }
                    if let Some(revision) = folder_revision {
                        if crate::filesystem::audiobook_fingerprint(&path, &audiobook_folder::discover(&path)?)? != revision {
                            return Err(AssetStoreError::operation("audiobook changed while preparing transfer"));
                        }
                    }
                    reader.seek(SeekFrom::Start(0))?;
                    return Ok(Some(VerifiedBook { reader, length: hasher.count(), book_id: actual, _lease: lease }));
                }
                Ok(None)
            })
            .await
            .map_err(AssetStoreError::operation)?
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn read(&self, kind: BlobKind, book_id: &BookId) -> Result<Option<Vec<u8>>, AssetStoreError> {
        imp::read(&self.0, asset_name(kind, book_id))
    }

    pub async fn read_bytes(&self, kind: BlobKind, hash: &BookId) -> Result<Option<Vec<u8>>, AssetStoreError> {
        self.read_named(asset_name(kind, hash)).await
    }

    pub async fn read_named(&self, mut name: String) -> Result<Option<Vec<u8>>, AssetStoreError> {
        let name = self
            .metadata(move |store| -> Result<_, AssetStoreError> {
                if let Some(generated) = generated_thumbnail_name(&name) {
                    let master = name.split('.').next().expect("thumbnail master");
                    let selected = imp::exists(&store.0, master.to_owned())?
                        && imp::exists(&store.0, format!("{master}.{}", thumbnail::card_thumbnail_suffix(thumbnail::BROWSE_THUMBNAIL_WIDTH)))?
                        && imp::exists(&store.0, format!("{master}.{}", thumbnail::card_thumbnail_suffix(thumbnail::THUMBNAIL_WIDTH)))?;
                    if !selected && imp::exists(&store.0, generated_thumbnail_name(master).expect("thumbnail name"))? {
                        name = generated;
                    }
                }
                Ok(name)
            })
            .await?;
        imp::read_bytes(&self.0, name).await
    }

    /// Retire a corrupt original only while it still contains the bytes that
    /// failed decoding. A concurrently selected replacement must survive.
    pub async fn discard_invalid_thumbnail_source(&self, hash: &BookId, expected: &[u8]) -> Result<(), AssetStoreError> {
        let _lease = self.try_exclusive_lease(hash)?.ok_or_else(|| AssetStoreError::operation("thumbnail is being updated"))?;
        let master = asset_name(BlobKind::Thumbnail, hash);
        if imp::read_bytes(&self.0, master.clone()).await?.as_deref() == Some(expected) {
            let hash = *hash;
            self.metadata(move |store| -> Result<(), AssetStoreError> {
                for name in [master, thumbnail_variant_name(&hash, thumbnail::BROWSE_THUMBNAIL_WIDTH), thumbnail_variant_name(&hash, thumbnail::THUMBNAIL_WIDTH)] {
                    imp::remove(&store.0, name)?;
                }
                Ok(())
            })
            .await?;
        }
        Ok(())
    }

    #[cfg(target_os = "android")]
    pub async fn open_playback_file(&self, relative_path: &RelativeBookPath) -> Result<Option<File>, AssetStoreError> {
        let store = self.clone();
        let relative_path = relative_path.clone();
        let root = self.lock_root_shared().await;
        run_blocking(move || {
            let _root = root;
            imp::open_playback_file(&store.0, &relative_path)
        })
        .await
        .map_err(AssetStoreError::operation)?
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn playback_reader(&self, hash: &BookId) -> Result<Option<client_platform_web::web_storage::FileReader>, AssetStoreError> {
        imp::open_verified_reader(&self.0, hash).await
    }

    /// Opens a native book at the selected library path. File readers keep the
    /// opened revision across atomic replacements; no lifetime book lease is
    /// retained. Callers coordinate opening with publication when also loading
    /// revision-specific metadata.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn open_book_at_path(&self, relative_path: RelativeBookPath) -> Result<Option<BoxedBookReader>, AssetStoreError> {
        let store = self.clone();
        run_blocking(move || imp::open_book_at_path(&store.0, &relative_path)).await.map_err(AssetStoreError::operation)?
    }

    /// Resolve a logical archive track through this folder's local name mapping.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn open_audiobook_track(&self, folder: RelativeBookPath, logical_name: String) -> Result<Option<BoxedBookReader>, AssetStoreError> {
        let store = self.clone();
        run_blocking(move || imp::open_audiobook_track(&store.0, &folder, &logical_name)).await.map_err(AssetStoreError::operation)?
    }

    /// Opens a pinned file for independent asynchronous document cursors.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn open_async_book_at_path(&self, relative_path: RelativeBookPath) -> Result<Option<book_access::AsyncBookSource>, AssetStoreError> {
        let store = self.clone();
        let root = self.lock_root_shared().await;
        run_blocking(move || {
            let _root = root;
            if store.is_retired() {
                return Ok(None);
            }
            let path = imp::placement_path(&store.0, &relative_path);
            if imp::import_in_progress(&path) {
                return Ok(None);
            }
            let file = match File::open(path) {
                Ok(file) => file,
                Err(error) => {
                    store.root_available();
                    return if error.kind() == ErrorKind::NotFound { Ok(None) } else { Err(error.into()) };
                }
            };
            book_access::AsyncBookSource::from_file(file).map(Some).map_err(Into::into)
        })
        .await
        .map_err(AssetStoreError::operation)?
    }

    pub async fn write(&self, kind: BlobKind, book_id: &BookId, bytes: &[u8]) -> Result<(), AssetStoreError> {
        imp::write(&self.0, asset_name(kind, book_id), bytes).await?;
        if kind == BlobKind::Thumbnail {
            let book_id = *book_id;
            self.metadata(move |store| -> Result<(), AssetStoreError> {
                imp::remove(&store.0, thumbnail_variant_name(&book_id, thumbnail::BROWSE_THUMBNAIL_WIDTH))?;
                imp::remove(&store.0, thumbnail_variant_name(&book_id, thumbnail::THUMBNAIL_WIDTH))?;
                Ok(())
            })
            .await?;
        }
        Ok(())
    }

    /// Limits an expendable thumbnail directory, including legacy size variants.
    /// Only use for libraries whose entire thumbnail directory is a cache.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn trim_thumbnail_cache(&self, max_bytes: u64) -> Result<(), AssetStoreError> {
        imp::update_thumbnail_cache(&self.0, max_bytes, None)
    }

    /// Evicts old thumbnails before publishing a source image within the limit.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn write_cached_thumbnail(&self, hash: BookId, bytes: Vec<u8>, max_bytes: u64) -> Result<(), AssetStoreError> {
        let store = self.clone();
        run_blocking(move || imp::update_thumbnail_cache(&store.0, max_bytes, Some((&hash, &bytes)))).await.map_err(AssetStoreError::operation)?
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub async fn stage_book(&self, relative: RelativeBookPath, mut source: Box<dyn std::io::Read + Send>) -> Result<CopiedBook, AssetStoreError> {
        let store = self.clone();
        let root = self.lock_root_shared().await;
        run_blocking(move || {
            let _root = root;
            if !store.root_available() {
                return Err(AssetStoreError::operation("library folder is unavailable"));
            }
            let directory = parent_directory(&imp::placement_path(&store.0, &relative))?.to_owned();
            let mut file = tempfile::Builder::new()
                .prefix(".bokheim-import.part-")
                .tempfile_in(&directory)
                .map_err(|error| AssetStoreError::operation(Error::new(error.kind(), format!("could not create an import file in library folder '{}': {error}", directory.display()))))?;
            let (checksum, size) = imp::write_import_bytes(&store.0, file.as_file_mut(), source.as_mut(), &directory)?;
            let copied_digest = Some((checksum, size, crate::filesystem::fingerprint(file.path())?));
            Ok(CopiedBook { inner: imp::CopiedBook { copied_digest, temporary_path: file.into_temp_path(), direct: None } })
        })
        .await
        .map_err(AssetStoreError::operation)?
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn begin_book_write(&self, relative: &RelativeBookPath, hash: &BookId) -> Result<AsyncAssetWriter, AssetStoreError> {
        let destination = imp::placement_path(&self.0, relative);
        let temporary = tempfile::Builder::new().prefix(".bokheim-download.part-").tempfile_in(parent_directory(&destination)?)?;
        let (file, temporary_path) = temporary.into_parts();
        let inner = imp::AsyncWriter { file: tokio::fs::File::from_std(file), temporary_path };
        Ok(AsyncAssetWriter { inner, file_name: relative.as_str().to_owned(), expected_hash: *hash, hasher: blake3::Hasher::new() })
    }

    /// Copies a picked browser file into book storage while hashing it.
    #[cfg(target_arch = "wasm32")]
    pub async fn stage_browser_file(&self, source: client_platform_web::transport::import_io::ImportSource) -> Result<StagedBook, AssetStoreError> {
        let handle = self.0.clone();
        client_platform_runtime::executor::storage_call(move || async move {
            let file = source.into_file().map_err(AssetStoreError::operation)?;
            Ok(StagedBook { inner: imp::stage_browser_file(&handle, &file).await? })
        })
        .await
        .map_err(AssetStoreError::operation)?
    }

    pub async fn stored_book_bytes(&self, book: BookId, paths: Vec<RelativeBookPath>) -> Result<u64, AssetStoreError> {
        imp::stored_book_bytes(&self.0, book, paths).await
    }

    /// Bytes in exactly the files that book eviction can remove.
    pub async fn evictable_book_bytes(&self, book: BookId, paths: Vec<RelativeBookPath>) -> Result<u64, AssetStoreError> {
        imp::evictable_book_bytes(&self.0, book, paths).await
    }

    pub async fn write_thumbnail_variant(&self, book_id: &BookId, width: u32, bytes: &[u8]) -> Result<(), AssetStoreError> {
        imp::write(&self.0, thumbnail_variant_name(book_id, width), bytes).await
    }

    /// Hold one exclusive lease through preparation and publication of the master
    /// and both card variants, so an upload cannot read mixed generations.
    pub async fn write_thumbnail_set(&self, hash: &BookId, bytes: &[u8], browse: &[u8], card_high_density: &[u8]) -> Result<(), AssetStoreError> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            imp::write_thumbnail_set(&self.0, hash, bytes, browse, card_high_density).await
        }
        #[cfg(target_arch = "wasm32")]
        {
            let store = self.clone();
            let hash = *hash;
            let (bytes, browse, card_high_density) = (bytes.to_owned(), browse.to_owned(), card_high_density.to_owned());
            storage_call(move || async move {
                let _lease = store.try_exclusive_lease(&hash)?.ok_or_else(|| AssetStoreError::operation("thumbnail is being updated"))?;
                store.write(BlobKind::Thumbnail, &hash, &bytes).await?;
                store.write_thumbnail_variant(&hash, thumbnail::BROWSE_THUMBNAIL_WIDTH, &browse).await?;
                store.write_thumbnail_variant(&hash, thumbnail::THUMBNAIL_WIDTH, &card_high_density).await
            })
            .await
            .map_err(AssetStoreError::operation)?
        }
    }

    pub async fn thumbnail_set_exists(&self, book_id: &BookId) -> Result<bool, AssetStoreError> {
        let owned_book = *book_id;
        #[cfg(not(target_arch = "wasm32"))]
        let root = self.lock_root_shared().await;
        self.metadata(move |store| {
            #[cfg(not(target_arch = "wasm32"))]
            let _root = root;
            let book_id = &owned_book;
            for generated in [false, true] {
                let names = [asset_name(BlobKind::Thumbnail, book_id), thumbnail_variant_name(book_id, thumbnail::BROWSE_THUMBNAIL_WIDTH), thumbnail_variant_name(book_id, thumbnail::THUMBNAIL_WIDTH)];
                let mut complete = true;
                for name in names {
                    let name = if generated { generated_thumbnail_name(&name).expect("thumbnail name") } else { name };
                    complete &= imp::exists(&store.0, name)?;
                }
                if complete {
                    return Ok(true);
                }
            }
            Ok(false)
        })
        .await
    }

    /// Generated files are separate from selected/downloaded artwork. They
    /// need no book lease, and a late job cannot overwrite a chosen cover.
    pub async fn write_generated_thumbnail(&self, hash: &BookId, versions: &thumbnail::ThumbnailVersions) -> Result<(), AssetStoreError> {
        // The master is written last and marks the complete generated set.
        let files = vec![
            (generated_thumbnail_name(&thumbnail_variant_name(hash, thumbnail::BROWSE_THUMBNAIL_WIDTH)).expect("thumbnail name"), versions.browse.clone()),
            (generated_thumbnail_name(&thumbnail_variant_name(hash, thumbnail::THUMBNAIL_WIDTH)).expect("thumbnail name"), versions.card_high_density.clone()),
            (generated_thumbnail_name(&asset_name(BlobKind::Thumbnail, hash)).expect("thumbnail name"), versions.high_density.clone()),
        ];
        #[cfg(not(target_arch = "wasm32"))]
        {
            imp::write_generated_thumbnail(&self.0, files).await
        }
        #[cfg(target_arch = "wasm32")]
        {
            let handle = self.0.clone();
            storage_call(move || async move {
                for (name, bytes) in files {
                    imp::write(&handle, name, &bytes).await?;
                }
                Ok(())
            })
            .await
            .map_err(AssetStoreError::operation)?
        }
    }

    pub async fn remove(&self, kind: BlobKind, book_id: &BookId) -> Result<bool, AssetStoreError> {
        let owned_book = *book_id;
        #[cfg(not(target_arch = "wasm32"))]
        let root = self.lock_root_shared().await;
        self.metadata(move |store| {
            #[cfg(not(target_arch = "wasm32"))]
            let _root = root;
            store.remove_files(kind, &owned_book)
        })
        .await
    }

    /// Remove cached assets after placement replay has finished. Native book
    /// files are removed by that replay; web book bytes share this batch.
    /// Return the exclusive lease so it also covers database confirmation.
    pub async fn purge_book_assets(&self, book_id: BookId, lease: BookLease) -> Result<BookLease, AssetStoreError> {
        let store = self.clone();
        #[cfg(not(target_arch = "wasm32"))]
        let root = self.lock_root_shared().await;
        run_blocking(move || {
            #[cfg(not(target_arch = "wasm32"))]
            let _root = root;
            #[cfg(not(target_arch = "wasm32"))]
            if !store.root_available() {
                return Err(AssetStoreError::operation("library folder is unavailable"));
            }
            #[cfg(target_arch = "wasm32")]
            store.remove_files(BlobKind::Book, &book_id)?;
            store.remove_files(BlobKind::Thumbnail, &book_id)?;
            Ok(lease)
        })
        .await
        .map_err(AssetStoreError::operation)?
    }

    fn remove_files(&self, kind: BlobKind, book_id: &BookId) -> Result<bool, AssetStoreError> {
        let removed = imp::remove(&self.0, asset_name(kind, book_id))?;
        if kind == BlobKind::Thumbnail {
            imp::remove(&self.0, thumbnail_variant_name(book_id, thumbnail::BROWSE_THUMBNAIL_WIDTH))?;
            imp::remove(&self.0, thumbnail_variant_name(book_id, thumbnail::THUMBNAIL_WIDTH))?;
            for name in [asset_name(kind, book_id), thumbnail_variant_name(book_id, thumbnail::BROWSE_THUMBNAIL_WIDTH), thumbnail_variant_name(book_id, thumbnail::THUMBNAIL_WIDTH)] {
                imp::remove(&self.0, generated_thumbnail_name(&name).expect("thumbnail name"))?;
            }
        }
        Ok(removed)
    }

    pub async fn exists(&self, kind: BlobKind, book_id: &BookId) -> Result<bool, AssetStoreError> {
        let owned_book = *book_id;
        #[cfg(not(target_arch = "wasm32"))]
        let root = self.lock_root_shared().await;
        self.metadata(move |store| {
            #[cfg(not(target_arch = "wasm32"))]
            let _root = root;
            let book_id = &owned_book;
            let name = asset_name(kind, book_id);
            if imp::exists(&store.0, name.clone())? {
                return Ok(true);
            }
            if kind == BlobKind::Thumbnail {
                return imp::exists(&store.0, generated_thumbnail_name(&name).expect("thumbnail name"));
            }
            Ok(false)
        })
        .await
    }

    /// Whether a library-relative placement path is a readable file. Lets the
    /// import orchestrator resolve materialization work from a database snapshot
    /// without holding the writer.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn placement_file_exists(&self, relative: &RelativeBookPath) -> bool {
        let path = imp::placement_path(&self.0, relative);
        !imp::import_in_progress(&path) && (path.is_file() || path.is_dir() && audiobook_folder::discover(&path).is_ok_and(|tracks| !tracks.is_empty()))
    }

    /// Best-effort filesystem ensure of a placement's parent directory, with
    /// no database involvement. Directory identity healing stays with the
    /// directory passes; this only keeps a download from failing on a
    /// missing folder that its snapshot still considers live.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn ensure_placement_parent(&self, relative: &RelativeBookPath) -> Result<(), AssetStoreError> {
        std::fs::create_dir_all(parent_directory(&imp::placement_path(&self.0, relative))?)?;
        Ok(())
    }

    /// Delete under the caller's exclusive lease, retaining it in blocking work
    /// if the caller is cancelled. Return it for the metadata commit afterward.
    pub async fn evict_book_files(&self, book: BookId, paths: Vec<RelativeBookPath>, lease: BookLease) -> Result<BookLease, AssetStoreError> {
        imp::evict_book_files(&self.0, book, paths, lease).await
    }

    /// Try to hold a book exclusively while publishing or removing bytes.
    pub fn try_exclusive_lease(&self, hash: &BookId) -> Result<Option<BookLease>, AssetStoreError> {
        imp::lease(&self.0, hash.to_hex().as_str(), true).map(|lease| lease.map(BookLease::new))
    }

    /// Hold across book and the referencing metadata commit. Cleanup
    /// skips leased hashes, including leases held by another native process.
    pub async fn lease_book(&self, hash: &BookId) -> Result<BookLease, AssetStoreError> {
        loop {
            if let Some(lease) = imp::lease(&self.0, hash.to_hex().as_str(), false)? {
                return Ok(BookLease::new(lease));
            }
            client_platform_runtime::executor::sleep(Duration::from_millis(25)).await.map_err(AssetStoreError::operation)?;
        }
    }
}

impl AssetStore {
    /// Count already-resolved native book paths. Resolving placements is
    /// database policy and deliberately belongs to the caller.
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn book_bytes_at_paths(&self, relative_paths: Vec<RelativeBookPath>) -> Result<u64, AssetStoreError> {
        imp::book_bytes_at_paths(&self.0, relative_paths).await
    }
}

pub struct VerifiedBook {
    #[cfg(not(target_arch = "wasm32"))]
    reader: BoxedBookReader,
    #[cfg(target_arch = "wasm32")]
    reader: client_platform_web::web_storage::FileReader,
    pub length: u64,
    pub book_id: BookId,
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

impl Read for VerifiedBook {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buffer)
    }
}
impl Seek for VerifiedBook {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.reader.seek(position)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl CopiedBook {
    pub async fn verify_transfer(self, expected: BookId) -> Result<StagedBook, AssetStoreError> {
        self.verify_transfer_bytes(expected.content_hash(), None).await
    }

    pub async fn verify_transfer_bytes(self, expected: Option<content_address::ContentHash>, expected_size: Option<u64>) -> Result<StagedBook, AssetStoreError> {
        if expected.is_none() && expected_size.is_none() {
            return Err(AssetStoreError::operation("transfer has no verified byte source"));
        }
        run_blocking(move || {
            let (checksum, size) = match self.inner.copied_digest {
                Some((checksum, size, fingerprint)) => {
                    if crate::filesystem::fingerprint(self.path())? != fingerprint {
                        return Err(AssetStoreError::operation("transferred book changed after copying"));
                    }
                    (checksum, size)
                }
                None => content_address::hash_reader_with_size(&mut File::open(self.path())?)?,
            };
            let actual = BookId::from(checksum);
            if expected.is_some_and(|digest| actual.content_hash() != Some(digest)) || expected_size.is_some_and(|length| length != size) {
                return Err(AssetStoreError::operation("transferred book failed content verification"));
            }
            Ok(self.into_staged(actual, size))
        })
        .await
        .map_err(AssetStoreError::operation)?
    }
}

pub struct BookLease {
    // The guard keeps the platform lease alive for this value's lifetime.
    _guard: imp::Lease,
}

impl BookLease {
    fn new(guard: imp::Lease) -> Self {
        Self { _guard: guard }
    }
}

/// A downloaded book being written to disk, hashed as it arrives.
#[cfg(not(target_arch = "wasm32"))]
pub struct AsyncAssetWriter {
    file_name: String,
    inner: imp::AsyncWriter,
    expected_hash: BookId,
    hasher: blake3::Hasher,
}

#[cfg(not(target_arch = "wasm32"))]
impl AsyncAssetWriter {
    pub async fn write_all(&mut self, buffer: &[u8]) -> Result<(), AssetStoreError> {
        let written = self.hasher.count();
        written.checked_add(buffer.len() as u64).ok_or_else(|| AssetStoreError::operation("asset length overflow"))?;
        self.inner.write_all(buffer).await?;
        self.hasher.update(buffer);
        Ok(())
    }

    pub async fn verify(self) -> Result<VerifiedAssetWriter, AssetStoreError> {
        let actual = BookId::from(self.hasher.finalize());
        if actual != self.expected_hash {
            log::warn!("file verification failed for {:?}: expected={} got={actual}", self.file_name, self.expected_hash);
            return Err(AssetStoreError::operation("file verification failed"));
        }
        Ok(VerifiedAssetWriter(self))
    }

    /// Verify a byte digest independently of the stable destination identity.
    pub async fn verify_checksum(self, checksum: content_address::ContentHash) -> Result<VerifiedAssetWriter, AssetStoreError> {
        if self.hasher.finalize().as_bytes() != checksum.as_bytes() {
            return Err(AssetStoreError::operation("download checksum does not match"));
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
        let checksum = content_address::ContentHash::from(self.0.hasher.finalize());
        // Byte verification already matched the requested content hash.
        let temporary_path = self.0.inner.finish_book().await?;
        Ok(StagedBook { inner: imp::StagedBook { byte_checksum: checksum, temporary_path, book_id: self.0.expected_hash, size_bytes: self.0.hasher.count(), direct: None } })
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

    /// Checksum, length and file revision observed when copying completed.
    /// Consumers must check the revision before reusing this byte evidence.
    pub fn copied_digest(&self) -> Option<(content_address::ContentHash, u64, u64)> {
        self.inner.copied_digest
    }

    pub fn open_reader(&self) -> Result<BoxedBookReader, AssetStoreError> {
        Ok(Box::new(File::open(self.path())?))
    }

    pub fn into_staged(self, book_id: BookId, size_bytes: u64) -> StagedBook {
        StagedBook {
            inner: imp::StagedBook {
                byte_checksum: book_id.content_hash().expect("copied bytes require an explicit checksum before assigning a book identity"),
                temporary_path: self.inner.temporary_path,
                direct: self.inner.direct,
                book_id,
                size_bytes,
            },
        }
    }
}

pub use library_model::PublishedStagedBook;

impl StagedBook {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_reader(&self) -> Result<BoxedBookReader, AssetStoreError> {
        Ok(Box::new(File::open(&self.inner.temporary_path)?))
    }

    pub fn byte_checksum(&self) -> content_address::ContentHash {
        self.inner.byte_checksum
    }

    /// Assign the logical book identity. Native publication durably records
    /// its byte checksum before making the staged file permanent.
    pub async fn with_local_identity(mut self, store: &AssetStore, identity: BookId) -> Result<Self, AssetStoreError> {
        if identity == self.inner.book_id {
            return Ok(self);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if !store.root_available() {
                return Err(AssetStoreError::operation("library folder is unavailable"));
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.inner.destination = imp::name(&store.0, format!("book/{identity}"));
        }
        self.inner.book_id = identity;
        Ok(self)
    }

    /// Record a stable identity's byte proof at the durability boundary, once
    /// per publication or retained upload rather than during intermediate steps.
    #[cfg(not(target_arch = "wasm32"))]
    fn remember_content_proof(&self, store: &AssetStore) -> Result<(), AssetStoreError> {
        let hash = self.book_id();
        if hash.content_hash().is_none() {
            imp::remember_local_checksum(&store.0, &hash, self.byte_checksum())?;
        }
        Ok(())
    }

    /// Publishes bytes from an immutable database snapshot. The caller records
    /// the resulting placement through the database acknowledgement API.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn publish_snapshot(self, store: &AssetStore, relative_path: &RelativeBookPath, occupied_names: &[String], audiobook_archive: bool) -> Result<PublishedStagedBook, AssetStoreError> {
        self.publish_snapshot_with_track_titles(store, relative_path, occupied_names, audiobook_archive, &[])
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn publish_snapshot_with_track_titles(
        self, store: &AssetStore, relative_path: &RelativeBookPath, occupied_names: &[String], audiobook_archive: bool, track_titles: &[(String, String)],
    ) -> Result<PublishedStagedBook, AssetStoreError> {
        if self.inner.direct.is_none() {
            let mut prepared = self.prepare_download(store, relative_path, occupied_names, audiobook_archive, track_titles)?;
            return prepared.publish(store, occupied_names);
        }
        // A direct import already wrote its bytes to the final destination.
        // Finalize that file without treating it as its own duplicate.
        self.remember_content_proof(store)?;
        let imp::StagedBook { temporary_path, direct, .. } = self.inner;
        let direct = direct.expect("direct import");
        let fingerprint = finalize_direct_import(temporary_path, &direct.path, |path| imp::sync_import_file(&store.0, path))?;
        let name = relative_path.file_name().to_owned();
        Ok(PublishedStagedBook { name, relative_path: relative_path.to_string(), published: true, fingerprint })
    }

    pub fn book_id(&self) -> BookId {
        self.inner.book_id
    }
    pub fn size_bytes(&self) -> u64 {
        self.inner.size_bytes
    }
    #[cfg(target_arch = "wasm32")]
    pub async fn open_reader(&self) -> Result<BoxedBookReader, AssetStoreError> {
        Ok(Box::new(self.inner.writer.open_reader().await.map_err(AssetStoreError::operation)?))
    }
    #[cfg(target_arch = "wasm32")]
    pub async fn commit(self) -> Result<(), AssetStoreError> {
        self.publish_pinned().await.map(drop)
    }

    /// Keep this exact published revision available for rejection until its
    /// database acknowledgement succeeds.
    #[cfg(target_arch = "wasm32")]
    pub async fn publish_pinned(self) -> Result<PublishedFile, AssetStoreError> {
        self.inner.writer.publish_pinned_as(self.inner.destination, self.inner.book_id).await.map_err(AssetStoreError::operation)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn finalize_direct_import(temporary_path: tempfile::TempPath, path: &Path, sync_parent: impl FnOnce(&Path) -> Result<(), AssetStoreError>) -> Result<Option<String>, AssetStoreError> {
    // Inspection has accepted these bytes. From here on a durability failure
    // must leave them available for discovery and a later retry.
    temporary_path.keep().map_err(|mut error| {
        error.path.disable_cleanup(true);
        AssetStoreError::operation(format!("could not finalize the imported book '{}': {}", path.display(), error.error))
    })?;
    sync_parent(path)?;
    published_fingerprint(path)
}

#[cfg(not(target_arch = "wasm32"))]
fn published_fingerprint(path: &Path) -> Result<Option<String>, AssetStoreError> {
    match file_fingerprint(path) {
        Ok(value) => Ok(Some(value.to_string())),
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => Err(error.into()),
        Err(error) => {
            // Fingerprints are a scan optimization. A completed publication can
            // be recorded without one and inspected again by the next scan.
            log::warn!("could not fingerprint published book '{}'; discovery will retry: {error}", path.display());
            Ok(None)
        }
    }
}

fn asset_name(kind: BlobKind, book_id: &BookId) -> String {
    format!("{}/{book_id}", kind.storage())
}

fn generated_thumbnail_name(name: &str) -> Option<String> {
    let name = name.strip_prefix("thumbnail/")?;
    let (book, suffix) = name.split_once('.').map_or((name, ""), |(book, suffix)| (book, suffix));
    // Local identities include a tag and are longer than content hashes.
    if book.parse::<BookId>().is_err() || (!suffix.is_empty() && !suffix.starts_with("card-v")) {
        return None;
    }
    let suffix = if suffix.is_empty() { String::new() } else { format!(".{suffix}") };
    Some(format!("thumbnail/{book}.generated-v{}{suffix}", thumbnail::CARD_THUMBNAIL_VERSION))
}

pub fn thumbnail_variant_name(book_id: &BookId, width: u32) -> String {
    format!("thumbnail/{book_id}.{}", thumbnail::card_thumbnail_suffix(width))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native as imp;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stable_identity_proof_is_written_only_at_publication_and_survives_reopen() {
        use content_address::ContentHash;
        use std::io::Cursor;
        for direct in [false, true] {
            let library = tempfile::tempdir().unwrap();
            let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
            let bytes = b"verified local import";
            let checksum = ContentHash::from(blake3::hash(bytes));
            let identity = BookId::for_local_content(checksum);
            let proof = store.0.assets_root.join("local-proofs").join(identity.to_hex());
            let (_, copied) = store.copy_import(path("book.epub"), Box::new(Cursor::new(bytes)), vec![], ImportOptions { direct }).await.unwrap();
            let staged = copied.into_staged(checksum.into(), bytes.len() as u64).with_local_identity(&store, identity).await.unwrap();
            assert!(!proof.exists(), "assigning identity must not publish a proof for uncommitted staging");
            let published = staged.publish_snapshot(&store, &path("book.epub"), &[], false).unwrap();
            assert!(published.published);
            assert_eq!(std::fs::read_to_string(&proof).unwrap(), format!("{checksum}\n"));
            drop(store);
            let reopened = AssetStore::open(library.path().to_str().unwrap()).unwrap();
            let destination = imp::placement_path(&reopened.0, &path("book.epub"));
            assert!(imp::content_matches(&destination, &identity).unwrap());
        }
    }

    #[tokio::test]
    async fn failed_content_proof_cannot_leave_a_permanent_import() {
        use content_address::ContentHash;
        use std::io::Cursor;
        for direct in [false, true] {
            let library = tempfile::tempdir().unwrap();
            let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
            let bytes = b"verified local import";
            let checksum = ContentHash::from(blake3::hash(bytes));
            let identity = BookId::for_local_content(checksum);
            let (_, copied) = store.copy_import(path("book.epub"), Box::new(Cursor::new(bytes)), vec![], ImportOptions { direct }).await.unwrap();
            let temporary = copied.path().to_owned();
            let staged = copied.into_staged(checksum.into(), bytes.len() as u64).with_local_identity(&store, identity).await.unwrap();
            // Force proof creation to fail before staging becomes permanent.
            std::fs::write(store.0.assets_root.join("local-proofs"), b"blocked").unwrap();
            assert!(staged.publish_snapshot(&store, &path("book.epub"), &[], false).is_err());
            assert!(!temporary.exists());
            assert!(!library.path().join("book.epub").exists());
        }
    }

    #[tokio::test]
    async fn verified_download_defers_its_proof_until_publication() {
        use content_address::ContentHash;
        {
            let library = tempfile::tempdir().unwrap();
            let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
            let bytes = b"downloaded revision";
            let checksum = ContentHash::from(blake3::hash(bytes));
            let identity = BookId::for_local_content(checksum);
            let proof = store.0.assets_root.join("local-proofs").join(identity.to_hex());
            let mut writer = store.begin_book_write(&path("book.epub"), &identity).unwrap();
            writer.write_all(bytes).await.unwrap();
            let verified = writer.verify_checksum(checksum).await.unwrap();
            let staged = verified.finish_book().await.unwrap();
            assert!(!proof.exists());
            staged.publish_snapshot(&store, &path("book.epub"), &[], false).unwrap();
            assert!(imp::content_matches(&library.path().join("book.epub"), &identity).unwrap());
        }
    }

    #[tokio::test]
    async fn copied_digest_does_not_accept_modified_staging() {
        let library = tempfile::tempdir().unwrap();
        let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let copied = store.stage_book(path("book.epub"), Box::new(std::io::Cursor::new(b"original"))).await.unwrap();
        std::fs::write(copied.path(), b"replaced").unwrap();
        let temporary = copied.path().to_owned();
        assert!(copied.verify_transfer_bytes(Some(blake3::hash(b"original").into()), Some(8)).await.is_err());
        assert!(!temporary.exists());
    }

    #[tokio::test]
    async fn transferred_books_require_the_supplied_checksum_and_length() {
        let library = tempfile::tempdir().unwrap();
        let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let bytes = b"verified transfer";
        let digest = content_address::ContentHash::from(blake3::hash(bytes));
        let wrong = content_address::ContentHash::from(blake3::hash(b"different bytes"));
        let length = bytes.len() as u64;
        for (checksum, size, accepted) in [(Some(digest), Some(length), true), (Some(digest), None, true), (None, Some(length), true), (Some(wrong), Some(length), false), (Some(digest), Some(length + 1), false), (None, None, false)] {
            let copied = store.stage_book(path("book.epub"), Box::new(std::io::Cursor::new(bytes))).await.unwrap();
            let temporary = copied.path().to_owned();
            let result = copied.verify_transfer_bytes(checksum, size).await;
            assert_eq!(result.is_ok(), accepted);
            if let Ok(staged) = result {
                assert_eq!(staged.book_id().content_hash(), Some(digest));
                assert_eq!(staged.size_bytes(), length);
            }
            assert!(!temporary.exists(), "finished verification must release the temporary file");
        }
    }

    #[test]
    fn a_disappeared_publication_is_not_recorded_as_successful() {
        let library = tempfile::tempdir().unwrap();
        assert!(published_fingerprint(&library.path().join("missing.epub")).is_err());
    }

    #[test]
    fn direct_import_survives_a_failed_directory_sync_and_can_be_finalized_again() {
        use std::io::{Error, Write};
        let library = tempfile::tempdir().unwrap();
        let mut file = tempfile::NamedTempFile::new_in(library.path()).unwrap();
        file.write_all(b"verified book").unwrap();
        let path = file.path().to_path_buf();
        let error = finalize_direct_import(file.into_temp_path(), &path, |_| Err(Error::from(ErrorKind::PermissionDenied).into())).unwrap_err();
        assert!(error.to_string().contains("permission denied"));
        assert_eq!(std::fs::read(&path).unwrap(), b"verified book");
        let retained = tempfile::TempPath::try_from_path(path.clone()).unwrap();
        assert!(finalize_direct_import(retained, &path, imp::sync_parent).unwrap().is_some());
        assert_eq!(std::fs::read(path).unwrap(), b"verified book");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fingerprint_failure_preserves_completed_imports_and_retry_does_not_duplicate_them() {
        use std::io::Cursor;
        use std::time::UNIX_EPOCH;
        for direct in [false, true] {
            let library = tempfile::tempdir().unwrap();
            let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
            let bytes = b"verified book";
            let hash = BookId::from(blake3::hash(bytes));
            let (_, copied) = store.copy_import(path("book.epub"), Box::new(Cursor::new(bytes)), vec![], ImportOptions { direct }).await.unwrap();
            // This filesystem can store a timestamp that the fingerprint
            // format cannot represent, while the book itself remains readable.
            File::options().write(true).open(copied.path()).unwrap().set_modified(UNIX_EPOCH - Duration::from_secs(1)).unwrap();
            assert!(file_fingerprint(copied.path()).is_err());
            let published = copied.into_staged(hash, bytes.len() as u64).publish_snapshot(&store, &path("book.epub"), &[], false).unwrap();
            assert!(published.published);
            assert!(published.fingerprint.is_none());
            assert_eq!(std::fs::read(library.path().join("book.epub")).unwrap(), bytes);

            let retry = store.stage_book(path("book.epub"), Box::new(Cursor::new(bytes))).await.unwrap().into_staged(hash, bytes.len() as u64);
            let published = retry.publish_snapshot(&store, &path("book.epub"), &[], false).unwrap();
            assert!(!published.published);
            assert_eq!(published.name, "book.epub");
            assert!(!library.path().join("book 2.epub").exists());
        }
    }

    fn path(value: &str) -> RelativeBookPath {
        RelativeBookPath::parse(value).unwrap()
    }

    fn nonportable_audio_archive() -> Vec<u8> {
        use std::io::{Cursor, Write};
        use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in [("NUL.mp3", b"opening".as_slice()), ("Disc?/track?.mp3", b"ending".as_slice()), ("chapters?.cue", b"FILE \"NUL.mp3\" MP3\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n".as_slice())] {
            zip.start_file(name, SimpleFileOptions::default().compression_method(CompressionMethod::Stored).system(zip::System::Unix)).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn windows_download_repairs_names_using_database_title_hints() {
        let library = tempfile::tempdir().unwrap();
        let archive = nonportable_audio_archive();
        let hash = BookId::from(blake3::hash(&archive));
        let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let mut writer = store.begin_book_write(&path("Story"), &hash).unwrap();
        writer.write_all(&archive).await.unwrap();
        let staged = writer.verify().await.unwrap().finish_book().await.unwrap();
        let published = staged.publish_snapshot_with_track_titles(&store, &path("Story"), &[], true, &[("NUL.mp3".into(), "Database chapter title".into())]).unwrap();
        assert!(published.published);
        assert_eq!(std::fs::read(library.path().join("Story/001 - Database chapter title.mp3")).unwrap(), b"opening");
        assert!(store.book_exists(&hash, &[path("Story")]).await.unwrap());
    }

    #[tokio::test]
    async fn foreign_audiobook_download_keeps_its_identity_through_copy_and_upload() {
        use std::io::{Cursor, Read, Write};
        use zip::{CompressionMethod, System, ZipWriter, write::SimpleFileOptions};
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored).system(if cfg!(windows) { System::Unix } else { System::Dos });
        for (name, bytes) in [("02.mp3", b"opening".as_slice()), ("01.mp3", b"ending".as_slice()), ("book.cue", b"FILE 02.mp3 MP3\n".as_slice()), ("book.nfo", b"Title: Story\n".as_slice())] {
            zip.start_file(name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        let archive = zip.finish().unwrap().into_inner();
        let hash = BookId::from(blake3::hash(&archive));
        let library = tempfile::tempdir().unwrap();
        let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let mut writer = store.begin_book_write(&path("Story"), &hash).unwrap();
        writer.write_all(&archive).await.unwrap();
        let staged = writer.verify().await.unwrap().finish_book().await.unwrap();
        let published = staged.publish_snapshot(&store, &path("Story"), &[], true).unwrap();
        assert!(published.published);
        assert!(store.book_exists(&hash, &[path("Story")]).await.unwrap());
        assert!(library.path().join("Story/02.mp3").is_file());
        store.prepare_placement(&hash, &path("Copy"), &[path("Story")], &[]).unwrap().unwrap().publish().unwrap();
        assert!(store.book_exists(&hash, &[path("Copy")]).await.unwrap());
        let mut verified = store.verified_book_reader_with_checksum(hash, vec![path("Copy")], None).await.unwrap().unwrap();
        let mut uploaded = Vec::new();
        verified.reader.read_to_end(&mut uploaded).unwrap();
        assert_eq!(uploaded, archive);
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
        let hash = BookId::from(audiobook_folder::folder_hash(source.path()).unwrap());
        let nfo = audiobook_folder::discover_nfo(source.path()).unwrap();
        let cue = audiobook_folder::discover_cue(source.path()).unwrap();
        let cover = audiobook_folder::discover_cover(source.path()).unwrap();
        let archive = audiobook_archive::write_archive_with_sidecars(std::io::Cursor::new(Vec::new()), &tracks, nfo.as_ref(), cue.as_ref(), cover.as_ref()).unwrap().into_inner();
        assert_eq!(hash, BookId::from(blake3::hash(&archive)));
        let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let mut writer = store.begin_book_write(&path("Story"), &hash).unwrap();
        writer.write_all(&archive).await.unwrap();
        let staged = writer.verify().await.unwrap().finish_book().await.unwrap();
        let published = staged.publish_snapshot(&store, &path("Story"), &[], true).unwrap();
        assert!(published.published);
        assert_eq!(published.name, "Story");
        assert_eq!(std::fs::read(library.path().join("Story/Disc 1/02.mp3")).unwrap(), b"ending");
        assert_eq!(std::fs::read(library.path().join("Story/book.nfo")).unwrap(), b"Title: Story\n");
        assert_eq!(std::fs::read(library.path().join("Story/book.cue")).unwrap(), b"FILE \"Disc 1/01.mp3\" MP3\nTRACK 01 AUDIO\nTITLE \"Opening\"\nINDEX 01 00:00:00\n");
        assert_eq!(std::fs::read(library.path().join("Story/cover.jpg")).unwrap(), b"cover fixture");
        assert!(store.book_exists(&hash, &[path("Story")]).await.unwrap());
        let verified = store.verified_book_reader_with_checksum(hash, vec![path("Story")], None).await.unwrap().unwrap();
        assert_eq!(verified.length, archive.len() as u64);
        assert_eq!(verified.book_id, hash);
        use futures_util::StreamExt as _;
        let result = verified.into_upload_stream().collect::<Vec<_>>().await.into_iter().collect::<Result<Vec<_>, _>>().unwrap().concat();
        assert_eq!(result, archive);
    }

    #[tokio::test]
    async fn chunked_downloads_publish_the_verified_length_across_storage_checks() {
        let library = tempfile::tempdir().unwrap();
        let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let bytes = (0..8 * 1024 * 1024 + 17).map(|index| (index % 251) as u8).collect::<Vec<_>>();
        let hash = BookId::from(blake3::hash(&bytes));
        let relative = path("chunked.epub");
        let mut writer = store.begin_book_write(&relative, &hash).unwrap();
        for chunk in bytes.chunks(64 * 1024) {
            writer.write_all(chunk).await.unwrap();
            writer.write_all(&[]).await.unwrap();
        }
        let staged = writer.verify().await.unwrap().finish_book().await.unwrap();
        assert_eq!(staged.size_bytes(), bytes.len() as u64);
        assert_eq!(staged.book_id(), hash);
        staged.publish_snapshot(&store, &relative, &[], false).unwrap();
        assert_eq!(std::fs::read(library.path().join("chunked.epub")).unwrap(), bytes);
    }

    #[tokio::test]
    async fn download_rejects_bytes_that_do_not_match_the_book_hash() {
        let library = tempfile::tempdir().unwrap();
        let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let hash = BookId::from(blake3::hash(b"expected book"));
        let mut writer = store.begin_book_write(&path("book.epub"), &hash).unwrap();
        writer.write_all(b"different book").await.unwrap();
        assert!(writer.verify().await.is_err());
        assert!(!library.path().join("book.epub").exists());
    }

    #[tokio::test]
    async fn thumbnail_set_includes_both_versioned_card_densities_and_invalidates_them_together() {
        let library = tempfile::tempdir().unwrap();
        let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let hash = BookId::from(blake3::hash(b"card variants"));
        store.write_thumbnail_set(&hash, b"original master", b"browse card", b"dense card").await.unwrap();
        assert!(store.thumbnail_set_exists(&hash).await.unwrap());
        assert_eq!(store.read_bytes(BlobKind::Thumbnail, &hash).await.unwrap().unwrap(), b"original master");
        for (width, expected) in [(300, b"browse card".as_slice()), (600, b"dense card".as_slice())] {
            let name = thumbnail_variant_name(&hash, width);
            assert!(name.contains(&format!("card-v{}", thumbnail::CARD_THUMBNAIL_VERSION)));
            assert_eq!(store.read_named(name).await.unwrap().unwrap(), expected);
        }
        store.write(BlobKind::Thumbnail, &hash, b"new original").await.unwrap();
        assert!(!store.thumbnail_set_exists(&hash).await.unwrap());
        for width in [300, 600] {
            assert!(store.read_named(thumbnail_variant_name(&hash, width)).await.unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn failed_thumbnail_book_preserves_the_browse_variant() {
        let library = tempfile::tempdir().unwrap();
        let store = AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let hash = BookId::from(blake3::hash(b"thumbnail owner"));
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
        let hash = BookId::from(blake3::hash(b"complete"));
        std::fs::write(library.path().join("book.epub"), b"existing user file").unwrap();
        for verify in [false, true] {
            let mut writer = store.begin_book_write(&path("/book.epub"), &hash).unwrap();
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
pub use native_placement::{DirectoryPlacement, PreparedDirectory, PreparedDownload, PreparedPlacement};
