//! Native library-file access and adjacent transfer staging.

use crate::APP_HIDDEN_DIR;
use crate::filesystem::{copy_entry, rename_entry};
use client_platform_native::filesystem::parent_directory;
use client_platform_runtime::executor::run_blocking;
use client_platform_runtime::profiling::ProfileSpan;
use content_address::{ContentHash, HashingReader, hash_file};
use fs2::{FileExt, lock_contended_error};
use std::fs::{File, OpenOptions};
use std::io::{Error, ErrorKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use tokio::sync::RwLock;
pub(crate) mod projection;
pub(crate) use projection::create_directory;
pub(crate) use projection::readable_path;

pub(crate) use super::{AssetStoreError, BookId, BookLease};
pub(crate) use book_access::BoxedBookReader;
pub(crate) use library_model::RelativeBookPath;
pub(crate) use std::collections::HashSet;
pub(crate) use std::path::{Path, PathBuf};
pub(crate) type RootFailure = Box<dyn FnOnce() + Send + 'static>;
#[derive(Clone)]
pub(crate) struct Handle {
    pub(crate) library_root: PathBuf,
    pub(crate) assets_root: PathBuf,
    pub(crate) storage_identity: String,
    pub(crate) leases_root: PathBuf,
    pub(crate) enabled: Arc<AtomicBool>,
    pub(crate) root_failure: Arc<Mutex<Option<RootFailure>>>,
    pub(crate) import_batch: Option<Arc<super::import_batch::PendingDurability>>,
    pub(crate) root_access: Arc<RwLock<()>>,
}

impl std::fmt::Debug for Handle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Handle").field("root", &self.assets_root).finish_non_exhaustive()
    }
}

pub(crate) fn upload_stream(book: super::VerifiedBook) -> impl futures_util::Stream<Item = Result<Vec<u8>, Error>> + Send {
    futures_util::stream::try_unfold((book, 0u64), |(mut book, sent)| async move {
        client_platform_runtime::executor::run_blocking(move || {
            use std::io::Read as _;
            let mut chunk = vec![0_u8; 64 * 1024];
            let length = book.reader.read(&mut chunk)?;
            let sent = sent + length as u64;
            if sent > book.length || (length == 0 && sent != book.length) {
                return Err(Error::other("book length changed while streaming"));
            }
            chunk.truncate(length);
            Ok::<_, Error>((length != 0).then_some((chunk, (book, sent))))
        })
        .await
        .map_err(Error::other)?
    })
}

pub(crate) struct Lease {
    // Closing the owned file releases its lock when the lease is dropped.
    _file: File,
}

pub(crate) fn lease(handle: &Handle, hash: &str, exclusive: bool) -> Result<Option<Lease>, AssetStoreError> {
    if !handle.enabled.load(Ordering::Acquire) {
        return Err(AssetStoreError::operation("library storage is unavailable"));
    }
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(handle.leases_root.join(hash)).inspect_err(|_| {
        root_available(handle);
    })?;
    let result = if exclusive { FileExt::try_lock_exclusive(&file) } else { FileExt::try_lock_shared(&file) };
    match result {
        Ok(()) => Ok(Some(Lease { _file: file })),
        Err(error) if error.kind() == ErrorKind::WouldBlock || error.raw_os_error().is_some_and(|code| Some(code) == lock_contended_error().raw_os_error()) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn open(library_locator: &str) -> Result<Handle, AssetStoreError> {
    #[cfg(target_os = "android")]
    let private_leases = Some(client_platform_android::book::lease_root()?);
    #[cfg(not(target_os = "android"))]
    let private_leases: Option<&Path> = None;
    let library_root = PathBuf::from(library_locator);
    let root = library_root.join(APP_HIDDEN_DIR).join("assets");
    // Create children one level at a time. Never recreate a library root
    // that disappears after its identity was checked.
    for directory in [parent_directory(&root)?.to_path_buf(), root.clone(), root.join("thumbnail")] {
        create_child_directory(&directory)?;
    }
    let marker = parent_directory(&root)?.join("storage-identity");
    let identity = match std::fs::read_to_string(&marker) {
        Ok(value) => value,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            use std::io::Write as _;
            let value = uuid::Uuid::new_v4().to_string();
            match OpenOptions::new().write(true).create_new(true).open(&marker) {
                Ok(mut file) => {
                    file.write_all(value.as_bytes())?;
                    file.sync_all()?;
                    value
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => std::fs::read_to_string(&marker)?,
                Err(error) => return Err(error.into()),
            }
        }
        Err(error) => return Err(error.into()),
    };
    // Android shared storage does not implement flock. Keep the same OS-backed
    // shared/exclusive locks on private storage, namespaced by canonical library
    // path so independently opened handles and processes coordinate correctly.
    let leases = match private_leases {
        Some(base) => base.join(blake3::hash(root.canonicalize()?.to_string_lossy().as_bytes()).to_hex().as_str()),
        None => root.join("leases"),
    };
    if private_leases.is_some() {
        std::fs::create_dir_all(&leases)?;
    } else {
        create_child_directory(&leases)?;
    }
    Ok(Handle { library_root, assets_root: root, storage_identity: identity, leases_root: leases, enabled: Arc::new(AtomicBool::new(true)), root_failure: Default::default(), import_batch: None, root_access: Default::default() })
}

pub(crate) fn root_available(handle: &Handle) -> bool {
    if !handle.enabled.load(Ordering::Acquire) {
        return false;
    }
    let available = std::fs::read_to_string(handle.library_root.join(APP_HIDDEN_DIR).join("storage-identity")).is_ok_and(|value| value == handle.storage_identity);
    if !available {
        let callback = handle.root_failure.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(callback) = callback {
            callback();
        }
    }
    available
}

pub(crate) fn placement_path(handle: &Handle, relative: &RelativeBookPath) -> PathBuf {
    library_path(handle, relative.as_str())
}

/// A library folder path: "" for the root, "a/b" below it.
pub(crate) fn library_path(handle: &Handle, relative: &str) -> PathBuf {
    handle.library_root.join(relative.trim_start_matches('/'))
}

pub(crate) fn m4b_sidecar_cover(handle: &Handle, paths: &[RelativeBookPath]) -> Result<Option<Vec<u8>>, AssetStoreError> {
    use std::io::Read as _;
    for relative in paths {
        let path = placement_path(handle, relative);
        if !path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("m4b")) {
            continue;
        }
        if let Some(cover) = audiobook_folder::discover_m4b_cover(&path)? {
            let mut bytes = Vec::new();
            File::open(cover.path)?.take(audiobook_archive::MAX_COVER_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > audiobook_archive::MAX_COVER_BYTES {
                return Err(AssetStoreError::operation("M4B cover exceeds byte limit"));
            }
            return Ok(Some(bytes));
        }
    }
    Ok(None)
}

pub(crate) fn content_matches(path: &Path, hash: &BookId) -> Result<bool, AssetStoreError> {
    let mut proofs = Vec::new();
    for ancestor in path.ancestors().skip(1) {
        let metadata = ancestor.join(APP_HIDDEN_DIR);
        if metadata.join("storage-identity").is_file() {
            let proof = metadata.join("assets").join("local-proofs").join(hash.to_hex());
            proofs = read_local_proof(&proof)?.lines().take(64).filter_map(|line| line.parse::<ContentHash>().ok()).collect();
            break;
        }
    }
    let matches = |actual: ContentHash| hash.content_hash() == Some(actual) || proofs.contains(&actual);
    if path.is_dir() {
        let tracks = audiobook_folder::discover(path)?;
        if tracks.is_empty() {
            return Ok(false);
        }
        let content = audiobook_folder::folder_content_with_reader(path, &tracks, |path| File::open(path))?;
        if matches(content.checksum.into()) {
            return Ok(true);
        }
        // Older downloads have only a transport-byte proof. Keep them usable
        // until scanning associates their recording fingerprint with the ID.
        return Ok(matches(audiobook_folder::folder_hash(path)?.into()));
    }
    Ok(matches(hash_file(&mut File::open(path)?)?))
}

fn read_local_proof(path: &Path) -> Result<String, AssetStoreError> {
    use std::io::Read;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => return Err(error.into()),
    };
    let mut proof = String::new();
    file.take(64 * 65 + 1).read_to_string(&mut proof)?;
    if proof.len() > 64 * 65 {
        return Err(AssetStoreError::operation("local download proof exceeds its size limit"));
    }
    Ok(proof)
}

pub(crate) fn remember_local_checksum(handle: &Handle, id: &BookId, checksum: ContentHash) -> Result<(), AssetStoreError> {
    use std::io::Write;

    let _call = ProfileSpan::new("checksum_proof_persistence", "book");
    let validation = ProfileSpan::new("checksum_proof_root_validation", "book");
    if !root_available(handle) {
        return Err(AssetStoreError::operation("library folder is unavailable"));
    }
    drop(validation);
    let directory = handle.assets_root.join("local-proofs");
    let creation = ProfileSpan::new("checksum_proof_directory_creation", "book");
    std::fs::create_dir_all(&directory)?;
    drop(creation);
    let path = directory.join(id.to_hex());
    let read = ProfileSpan::new("checksum_proof_read", "book");
    let proof = read_local_proof(&path)?;
    drop(read);
    let parsing = ProfileSpan::new("checksum_proof_parse", "book");
    let mut known = proof.lines().filter_map(|line| line.parse::<ContentHash>().ok()).take(63).collect::<Vec<_>>();
    known.retain(|known| *known != checksum);
    known.insert(0, checksum);
    drop(parsing);
    let creation = ProfileSpan::new("checksum_proof_tempfile", "book");
    let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
    drop(creation);
    let write = ProfileSpan::new("checksum_proof_write", "book");
    for digest in known {
        writeln!(temporary, "{digest}")?;
    }
    drop(write);
    if handle.import_batch.is_none() {
        let _sync = ProfileSpan::new("checksum_proof_file_sync", "book");
        temporary.as_file().sync_all()?;
    }
    let rename = ProfileSpan::new("checksum_proof_rename", "book");
    temporary.persist(&path).map_err(|error| AssetStoreError::operation(error.error))?;
    drop(rename);
    let _sync = ProfileSpan::new("checksum_proof_directory_sync", "book");
    sync_import_file(handle, &path)
}

pub(crate) fn placement_conflicts(handle: &Handle, hash: &BookId, relative: &RelativeBookPath) -> Result<bool, AssetStoreError> {
    let path = placement_path(handle, relative);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if !(metadata.is_file() || metadata.is_dir()) || metadata.file_type().is_symlink() => Ok(true),
        Ok(_) => Ok(!content_matches(&path, hash)?),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

// Hashing and copying happen before taking SQLite's writer. The final file
// action checks that verified paths have not changed while preparing the job.
pub(crate) struct VerifiedPath {
    pub(crate) path: PathBuf,
    pub(crate) hash: BookId,
    pub(crate) directory: bool,
    pub(crate) length: u64,
    pub(crate) modified: std::time::SystemTime,
    revision: u64,
}

impl VerifiedPath {
    pub(crate) fn new(path: PathBuf, hash: &BookId) -> Result<Self, AssetStoreError> {
        Self::matching(path, hash)?.ok_or_else(|| AssetStoreError::operation("file contains different book bytes"))
    }

    /// A different identity is distinct from an unreadable/invalid placement.
    /// Only cleanup may treat that identity mismatch as obsolete work.
    pub(crate) fn matching(path: PathBuf, hash: &BookId) -> Result<Option<Self>, AssetStoreError> {
        if import_in_progress(&path) {
            return Err(AssetStoreError::operation("book publication is still being prepared"));
        }
        Self::matching_unpublished(path, hash)
    }

    fn matching_unpublished(path: PathBuf, hash: &BookId) -> Result<Option<Self>, AssetStoreError> {
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !(metadata.is_file() || metadata.is_dir()) {
            return Err(AssetStoreError::operation("unsupported book placement"));
        }
        let revision = placement_revision(&path, metadata.is_dir())?;
        let verified = Self { path, hash: *hash, directory: metadata.is_dir(), length: metadata.len(), modified: metadata.modified()?, revision };
        if !content_matches(&verified.path, hash)? {
            return Ok(None);
        }
        verified.check()?;
        Ok(Some(verified))
    }

    pub(crate) fn check(&self) -> Result<(), AssetStoreError> {
        self.check_metadata()?;
        if self.directory && !content_matches(&self.path, &self.hash)? {
            return Err(AssetStoreError::operation("book changed while preparing file work"));
        }
        Ok(())
    }

    pub(crate) fn check_metadata(&self) -> Result<(), AssetStoreError> {
        let metadata = std::fs::symlink_metadata(&self.path)?;
        if metadata.is_dir() != self.directory || metadata.file_type().is_symlink() || metadata.len() != self.length || metadata.modified()? != self.modified || placement_revision(&self.path, self.directory)? != self.revision {
            return Err(AssetStoreError::operation("book changed while preparing file work"));
        }
        Ok(())
    }
}

/// Uses the same revision observations as scanning, including track contents'
/// metadata and file identities. It never reads all book bytes.
fn placement_revision(path: &Path, directory: bool) -> Result<u64, AssetStoreError> {
    if directory { Ok(crate::filesystem::audiobook_fingerprint(path, &audiobook_folder::discover(path)?)?) } else { Ok(crate::filesystem::fingerprint(path)?) }
}

pub(crate) enum PreparedPlacement {
    /// Expected book is absent; the path may contain an unrelated replacement.
    Absent,
    Existing(VerifiedPath),
    Copied(PreparedFile),
    CopiedFolder {
        output: tempfile::TempDir,
        destination: PathBuf,
    },
    /// A move whose source is also queued for removal: nothing else can need
    /// it, so relocate instead of copying.
    Renamed {
        source: VerifiedPath,
        destination: PathBuf,
    },
    Removed(VerifiedPath),
}

impl PreparedPlacement {
    pub(crate) fn publish(self) -> Result<(), AssetStoreError> {
        self.publish_with_copy_fallback(true)
    }

    pub(crate) fn publish_short(self) -> Result<(), AssetStoreError> {
        self.publish_with_copy_fallback(false)
    }

    fn publish_with_copy_fallback(self, allow_copy: bool) -> Result<(), AssetStoreError> {
        match self {
            Self::Absent => Ok(()),
            Self::Existing(file) => {
                if allow_copy {
                    file.check()
                } else {
                    file.check_metadata()
                }
            }
            Self::Copied(file) => {
                let destination = file.destination.clone();
                file.publish()?;
                sync_parent(&destination)
            }
            Self::CopiedFolder { output, destination } => {
                if destination.exists() {
                    return Err(AssetStoreError::operation("placement destination changed"));
                }
                rename_entry(output.path(), &destination)?;
                sync_parent(&destination)
            }
            Self::Renamed { source, destination } => {
                let verified = source;
                verified.check_metadata()?;
                let source = verified.path.clone();
                if destination.exists() {
                    return Err(AssetStoreError::operation("placement destination changed"));
                }
                match rename_entry(&source, &destination) {
                    Ok(()) => {}
                    Err(error) if allow_copy && error.kind() == ErrorKind::CrossesDevices => {
                        // Stage the whole copy before publication. A failed copy
                        // leaves both the source and destination untouched.
                        let staging = tempfile::Builder::new().prefix(".bokheim-copy-").tempdir_in(parent_directory(&destination)?)?;
                        let staged = staging.path().join("contents");
                        copy_entry(&source, &staged)?;
                        if !content_matches(&staged, &verified.hash)? {
                            return Err(AssetStoreError::operation("book changed during copy"));
                        }
                        verified.check()?;
                        rename_entry(&staged, &destination)?;
                        if source.is_dir() {
                            std::fs::remove_dir_all(&source)?;
                        } else {
                            std::fs::remove_file(&source)?;
                        }
                    }
                    Err(error) => return Err(error.into()),
                }
                sync_parent(&destination)?;
                sync_parent(&source)
            }
            Self::Removed(source) => {
                if allow_copy {
                    source.check()?;
                } else {
                    source.check_metadata()?;
                }
                if source.directory {
                    std::fs::remove_dir_all(&source.path)?;
                } else {
                    std::fs::remove_file(&source.path)?;
                }
                sync_parent(&source.path)
            }
        }
    }
}

pub(crate) fn prepare_removal(handle: &Handle, hash: &BookId, relative: &RelativeBookPath) -> Result<PreparedPlacement, AssetStoreError> {
    let source = placement_path(handle, relative);
    match std::fs::symlink_metadata(&source) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(PreparedPlacement::Absent),
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    // The expected book is no longer at this path. Acknowledge only this
    // book's obsolete work/projection without touching the replacement bytes.
    let Some(source) = VerifiedPath::matching(source, hash)? else { return Ok(PreparedPlacement::Absent) };
    Ok(PreparedPlacement::Removed(source))
}

pub(crate) struct AsyncWriter {
    // Field order closes the handle before removing an abandoned temporary file.
    pub(crate) file: tokio::fs::File,
    pub(crate) temporary_path: tempfile::TempPath,
}

impl AsyncWriter {
    pub(crate) async fn write_all(&mut self, buffer: &[u8]) -> Result<(), AssetStoreError> {
        use tokio::io::AsyncWriteExt as _;
        self.file.write_all(buffer).await?;
        Ok(())
    }

    pub(crate) async fn finish_book(mut self) -> Result<tempfile::TempPath, AssetStoreError> {
        use tokio::io::AsyncWriteExt as _;
        self.file.flush().await?;
        self.file.sync_all().await?;
        drop(self.file);
        Ok(self.temporary_path)
    }
}

pub(crate) fn sync_parent(path: &Path) -> Result<(), AssetStoreError> {
    #[cfg(unix)]
    {
        let parent = path.parent().ok_or_else(|| AssetStoreError::operation("asset has no parent"))?;
        client_platform_native::filesystem::sync_directory(parent).map_err(|error| AssetStoreError::operation(Error::new(error.kind(), format!("could not sync changes to library folder '{}': {error}", parent.display()))))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

pub(crate) fn sync_import_file(handle: &Handle, path: &Path) -> Result<(), AssetStoreError> {
    if let Some(batch) = &handle.import_batch {
        batch.record(path);
        Ok(())
    } else {
        sync_parent(path)
    }
}

#[cfg(target_os = "android")]
pub(crate) use client_platform_android::book::persist_book_noclobber;

#[cfg(not(target_os = "android"))]
pub(crate) fn persist_book_noclobber(temporary: tempfile::TempPath, destination: &Path) -> Result<(), tempfile::PathPersistError> {
    client_platform_native::filesystem::persist_noclobber(temporary, destination)
}

/// A file whose expensive storage work is complete. Android may have to copy
/// instead of rename on shared storage; do that during preparation, retaining
/// ownership of the destination until the guarded metadata commit.
pub(crate) struct PreparedFile {
    temporary: Option<tempfile::TempPath>,
    destination: PathBuf,
    #[cfg(target_os = "android")]
    verified: VerifiedPath,
    #[cfg(target_os = "android")]
    _direct: DirectBook,
}

impl PreparedFile {
    pub(crate) fn prepare(temporary: tempfile::TempPath, destination: PathBuf, hash: &BookId) -> Result<Self, AssetStoreError> {
        #[cfg(target_os = "android")]
        {
            let name = destination.file_name().ok_or_else(|| AssetStoreError::operation("book destination has no filename"))?;
            let destination = parent_directory(&destination)?.canonicalize()?.join(name);
            let imports = DIRECT_IMPORTS.get_or_init(Default::default);
            if !imports.lock().unwrap_or_else(PoisonError::into_inner).insert(destination.clone()) {
                return Err(AssetStoreError::operation("book destination is being prepared"));
            }
            let direct = DirectBook { path: destination.clone() };
            // This is the only Android compatibility-copy call. Preparation
            // runs off the library thread without the background job's gate.
            persist_book_noclobber(temporary, &destination).map_err(AssetStoreError::operation)?;
            let temporary = tempfile::TempPath::try_from_path(&destination)?;
            let verified = VerifiedPath::matching_unpublished(destination.clone(), hash)?.ok_or_else(|| AssetStoreError::operation("prepared file contains different book bytes"))?;
            Ok(Self { temporary: Some(temporary), destination, verified, _direct: direct })
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = hash;
            Ok(Self { temporary: Some(temporary), destination })
        }
    }

    /// Only a short rename, or adoption of Android's already prepared file.
    /// Never copy file contents while the caller holds the library gate.
    pub(crate) fn publish(mut self) -> Result<(), AssetStoreError> {
        #[cfg(target_os = "android")]
        {
            self.verified.check_metadata()?;
            self.temporary.take().expect("unpublished file").keep().map_err(AssetStoreError::operation)?;
            Ok(())
        }
        #[cfg(not(target_os = "android"))]
        {
            persist_book_noclobber(self.temporary.take().expect("unpublished file"), &self.destination).map_err(AssetStoreError::operation)
        }
    }
}

#[cfg(target_os = "android")]
impl Drop for PreparedFile {
    fn drop(&mut self) {
        if let Some(mut temporary) = self.temporary.take() {
            // A folder move or an external edit can replace this pathname
            // while preparation waits for admission. Only remove our revision.
            if self.verified.check_metadata().is_err() {
                temporary.disable_cleanup(true);
            }
        }
    }
}

pub(crate) struct StagedBook {
    pub(crate) byte_checksum: ContentHash,
    // A cleanup guard; direct imports use the final path from the outset.
    pub(crate) temporary_path: tempfile::TempPath,
    pub(crate) book_id: BookId,
    pub(crate) size_bytes: u64,
    pub(crate) direct: Option<DirectBook>,
}

pub(crate) struct CopiedBook {
    pub(crate) copied_digest: Option<(ContentHash, u64, u64)>,
    pub(crate) temporary_path: tempfile::TempPath,
    pub(crate) direct: Option<DirectBook>,
}

pub(crate) static DIRECT_IMPORTS: OnceLock<std::sync::Mutex<HashSet<PathBuf>>> = OnceLock::new();

pub(crate) struct DirectBook {
    pub(crate) path: PathBuf,
}

impl Drop for DirectBook {
    fn drop(&mut self) {
        if let Some(imports) = DIRECT_IMPORTS.get() {
            imports.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.path);
        }
    }
}

pub(crate) fn import_in_progress(path: &Path) -> bool {
    let Some(imports) = DIRECT_IMPORTS.get() else { return false };
    let imports = imports.lock().unwrap_or_else(PoisonError::into_inner);
    !imports.is_empty() && path.canonicalize().is_ok_and(|path| imports.contains(&path))
}

pub(crate) fn write_import_bytes(handle: &Handle, file: &mut File, source: &mut (dyn std::io::Read + Send), directory: &Path) -> Result<(ContentHash, u64), AssetStoreError> {
    use std::io::{Read as _, Write as _};
    let mut source = HashingReader::new(source);
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = match source.read(&mut buffer) {
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            result => result.map_err(|error| AssetStoreError::operation(Error::new(error.kind(), format!("could not read the selected book: {error}"))))?,
        };
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read]).map_err(|error| AssetStoreError::operation(Error::new(error.kind(), format!("could not write the imported book in library folder '{}': {error}", directory.display()))))?;
    }
    if source.size_bytes() == 0 {
        return Err(AssetStoreError::operation("book is empty".to_owned()));
    }
    #[cfg(target_os = "linux")]
    if let Some(batch) = &handle.import_batch {
        batch.queue_writeback(file, source.size_bytes());
    }
    if handle.import_batch.is_none() {
        file.sync_all().map_err(|error| AssetStoreError::operation(Error::new(error.kind(), format!("could not flush the imported book in library folder '{}': {error}", directory.display()))))?;
    }
    Ok((source.checksum(), source.size_bytes()))
}

pub(crate) async fn evict_book_files(handle: &Handle, _book: BookId, paths: Vec<RelativeBookPath>, lease: BookLease) -> Result<BookLease, AssetStoreError> {
    let root_access = handle.root_access.clone().read_owned().await;
    let handle = handle.clone();
    run_blocking(move || {
        let _root = root_access;
        projection::evict_book_files(&handle, &paths)?;
        Ok(lease)
    })
    .await
    .map_err(AssetStoreError::operation)?
}

pub(crate) async fn evictable_book_bytes(handle: &Handle, _book: BookId, paths: Vec<RelativeBookPath>) -> Result<u64, AssetStoreError> {
    let handle = handle.clone();
    client_platform_runtime::executor::run_blocking(move || {
        let mut total = 0_u64;
        let mut counted = HashSet::new();
        for relative in paths {
            for path in projection::book_eviction_files(&placement_path(&handle, &relative))? {
                if counted.insert(path.clone()) {
                    total = total.saturating_add(std::fs::symlink_metadata(path)?.len());
                }
            }
        }
        Ok(total)
    })
    .await
    .map_err(AssetStoreError::operation)?
}

pub(crate) async fn stored_book_bytes(handle: &Handle, _book: BookId, paths: Vec<RelativeBookPath>) -> Result<u64, AssetStoreError> {
    book_bytes_at_paths(handle, paths).await
}

pub(crate) async fn book_bytes_at_paths(handle: &Handle, paths: Vec<RelativeBookPath>) -> Result<u64, AssetStoreError> {
    let handle = handle.clone();
    client_platform_runtime::executor::run_blocking(move || {
        let mut total = 0_u64;
        let mut counted = HashSet::new();
        for relative in paths {
            let path = placement_path(&handle, &relative);
            let metadata = match std::fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if metadata.is_file() && counted.insert(path.clone()) {
                total = total.saturating_add(metadata.len());
            } else if metadata.is_dir() {
                for entry in walkdir::WalkDir::new(path).follow_links(false) {
                    let entry = entry.map_err(AssetStoreError::operation)?;
                    if entry.file_type().is_file() && counted.insert(entry.path().to_owned()) {
                        total = total.saturating_add(entry.metadata().map_err(AssetStoreError::operation)?.len());
                    }
                }
            }
        }
        Ok(total)
    })
    .await
    .map_err(AssetStoreError::operation)?
}

#[cfg(target_os = "android")]
pub(crate) fn open_playback_file(handle: &Handle, relative_path: &RelativeBookPath) -> Result<Option<File>, AssetStoreError> {
    if !handle.enabled.load(Ordering::Acquire) {
        return Ok(None);
    }
    let path = placement_path(handle, relative_path);
    if import_in_progress(&path) {
        return Ok(None);
    }
    match File::open(path) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn open_audiobook_track(handle: &Handle, folder: &RelativeBookPath, logical_name: &str) -> Result<Option<BoxedBookReader>, AssetStoreError> {
    if !handle.enabled.load(Ordering::Acquire) {
        return Ok(None);
    }
    let folder = placement_path(handle, folder);
    if !folder.try_exists()? {
        root_available(handle);
        return Ok(None);
    }
    open_file_reader(&audiobook_folder::resolve_track_path(&folder, logical_name)?)
}

pub(crate) fn open_book_at_path(handle: &Handle, relative_path: &RelativeBookPath) -> Result<Option<BoxedBookReader>, AssetStoreError> {
    if !handle.enabled.load(Ordering::Acquire) {
        return Ok(None);
    }
    let path = placement_path(handle, relative_path);
    if path.is_dir() {
        let tracks = audiobook_folder::discover(&path)?;
        if tracks.is_empty() {
            return Ok(None);
        }
        let archive = audiobook_folder::folder_archive_reader(&path, &tracks, |path| File::open(path))?;
        return Ok(Some(Box::new(archive)));
    }
    let result = open_file_reader(&path);
    if !matches!(&result, Ok(Some(_))) {
        root_available(handle);
    }
    result
}

fn open_file_reader(path: &Path) -> Result<Option<BoxedBookReader>, AssetStoreError> {
    if import_in_progress(path) {
        return Ok(None);
    }
    match File::open(path) {
        Ok(file) => Ok(Some(Box::new(file))),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn read(handle: &Handle, name: String) -> Result<Option<Vec<u8>>, AssetStoreError> {
    let Some(path) = readable_path(handle, name)? else { return Ok(None) };
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

/// Read contents asynchronously after resolving their library-relative path.
pub(crate) async fn read_bytes(handle: &Handle, name: String) -> Result<Option<Vec<u8>>, AssetStoreError> {
    let Some(path) = readable_path(handle, name)? else { return Ok(None) };
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn update_thumbnail_cache(handle: &Handle, max_bytes: u64, incoming: Option<(&BookId, &[u8])>) -> Result<(), AssetStoreError> {
    use std::{fs, io::Write};

    if !root_available(handle) {
        return Err(AssetStoreError::operation("library folder is unavailable"));
    }
    // Serialize pruning and publication across handles and processes, while
    // callers download and decode different covers concurrently.
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(handle.leases_root.join("thumbnail-cache"))?;
    FileExt::lock_exclusive(&lock)?;
    if !root_available(handle) {
        return Err(AssetStoreError::operation("library folder is unavailable"));
    }
    let reserved = incoming.map_or(0, |(_, bytes)| bytes.len() as u64);
    let available = max_bytes.checked_sub(reserved).ok_or_else(|| AssetStoreError::operation("thumbnail exceeds cache limit"))?;
    let directory = handle.assets_root.join("thumbnail");
    let mut files = Vec::new();
    let mut total = 0_u64;
    for entry in fs::read_dir(&directory)? {
        let entry = entry?;
        // Never follow symlinks or recurse into other storage.
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.is_file() {
            total = total.saturating_add(metadata.len());
            files.push((metadata.modified()?, entry.path(), metadata.len()));
        }
    }
    files.sort_unstable_by_key(|(modified, _, _)| *modified);
    for (_, path, size) in files {
        if total <= available {
            break;
        }
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        total = total.saturating_sub(size);
    }
    if let Some((hash, bytes)) = incoming {
        // Reserve space before staging, so even the temporary write fits.
        let mut file = tempfile::Builder::new().prefix(".bokheim-thumbnail.part-").tempfile_in(&directory)?;
        file.write_all(bytes)?;
        file.as_file().sync_all()?;
        let destination = directory.join(hash.to_hex());
        file.persist(&destination).map_err(|error| error.error)?;
        sync_parent(&destination)?;
    }
    Ok(())
}

pub(crate) async fn write(handle: &Handle, name: String, bytes: &[u8]) -> Result<(), AssetStoreError> {
    if name.starts_with("book/") {
        return Err(AssetStoreError::operation("native books require a library destination"));
    }
    let hash = name.strip_prefix("thumbnail/").and_then(|name| name.split('.').next()).ok_or_else(|| AssetStoreError::operation("unsupported native asset path"))?;
    let Some(lease) = lease(handle, hash, false)? else { return Err(AssetStoreError::operation("book is busy; retry thumbnail write")) };
    write_files(handle, vec![(name, bytes.to_owned())], Some(lease)).await
}

pub(crate) async fn write_thumbnail_set(handle: &Handle, hash: &BookId, bytes: &[u8], browse: &[u8], card_high_density: &[u8]) -> Result<(), AssetStoreError> {
    let Some(lease) = lease(handle, hash.to_hex().as_str(), true)? else { return Err(AssetStoreError::operation("thumbnail is being updated")) };
    write_files(
        handle,
        vec![
            (super::asset_name(super::BlobKind::Thumbnail, hash), bytes.to_owned()),
            (super::thumbnail_variant_name(hash, thumbnail::BROWSE_THUMBNAIL_WIDTH), browse.to_owned()),
            (super::thumbnail_variant_name(hash, thumbnail::THUMBNAIL_WIDTH), card_high_density.to_owned()),
        ],
        Some(lease),
    )
    .await
}

pub(crate) async fn write_generated_thumbnail(handle: &Handle, files: Vec<(String, Vec<u8>)>) -> Result<(), AssetStoreError> {
    write_files(handle, files, None).await
}

async fn write_files(handle: &Handle, files: Vec<(String, Vec<u8>)>, lease: Option<Lease>) -> Result<(), AssetStoreError> {
    let root = handle.root_access.clone().read_owned().await;
    let handle = handle.clone();
    run_blocking(move || {
        use std::io::Write as _;
        let _access = (root, lease);
        if !root_available(&handle) {
            return Err(AssetStoreError::operation("library folder is unavailable"));
        }
        let mut prepared = Vec::with_capacity(files.len());
        for (name, bytes) in files {
            let destination = handle.assets_root.join(name);
            let mut file = tempfile::Builder::new().prefix(".bokheim-thumbnail.part-").tempfile_in(parent_directory(&destination)?)?;
            file.write_all(&bytes)?;
            file.as_file().sync_all()?;
            prepared.push((file.into_temp_path(), destination));
        }
        if !root_available(&handle) {
            return Err(AssetStoreError::operation("library folder is unavailable"));
        }
        // Prepare every variant before publishing the set under its lease.
        for (temporary, destination) in prepared {
            temporary.persist(&destination).map_err(|error| error.error)?;
            sync_parent(&destination)?;
        }
        Ok(())
    })
    .await
    .map_err(AssetStoreError::operation)?
}

pub(crate) fn remove(handle: &Handle, name: String) -> Result<bool, AssetStoreError> {
    if !handle.enabled.load(Ordering::Acquire) {
        return Err(AssetStoreError::operation("library storage is unavailable"));
    }
    if name.starts_with("book/") {
        return Err(AssetStoreError::operation("native books require explicit paths"));
    }
    match std::fs::remove_file(handle.assets_root.join(name)) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn exists(handle: &Handle, name: String) -> Result<bool, AssetStoreError> {
    Ok(readable_path(handle, name)?.is_some())
}

#[cfg(test)]
mod lease_location_tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn missing_files_report_root_loss_once_and_leave_healthy_roots_alone() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let library = tempfile::tempdir().unwrap();
        let store = super::super::AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let failures = Arc::new(AtomicUsize::new(0));
        let reported = failures.clone();
        store.on_root_unavailable(move || {
            reported.fetch_add(1, Ordering::Relaxed);
        });
        let hash = BookId::from(blake3::hash(b"missing book"));
        let paths = [RelativeBookPath::parse("book.epub").unwrap()];

        assert!(!store.book_exists(&hash, &paths).await.unwrap());
        assert!(store.read(library_model::BlobKind::Thumbnail, &hash).unwrap().is_none());
        assert_eq!(failures.load(Ordering::Relaxed), 0, "missing individual files must not trigger root recovery");

        std::fs::remove_dir_all(library.path().join(APP_HIDDEN_DIR)).unwrap();
        assert!(!store.book_exists(&hash, &paths).await.unwrap());
        assert_eq!(failures.load(Ordering::Relaxed), 1);
        assert!(!store.root_available());
        assert!(!store.book_exists(&hash, &paths).await.unwrap());
        assert_eq!(failures.load(Ordering::Relaxed), 1, "the unavailable store must not flood the recovery queue");
    }

    #[tokio::test]
    async fn a_returning_mount_reports_its_changed_identity_even_when_the_book_exists() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let library = tempfile::tempdir().unwrap();
        let store = super::super::AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let failures = Arc::new(AtomicUsize::new(0));
        let reported = failures.clone();
        store.on_root_unavailable(move || {
            reported.fetch_add(1, Ordering::Relaxed);
        });
        let hash = BookId::from(blake3::hash(b"book"));
        let paths = [RelativeBookPath::parse("book.epub").unwrap()];
        std::fs::write(library.path().join("book.epub"), b"book").unwrap();
        assert!(store.book_exists(&hash, &paths).await.unwrap());
        assert_eq!(failures.load(Ordering::Relaxed), 0);
        std::fs::write(library.path().join(APP_HIDDEN_DIR).join("storage-identity"), b"returned mount").unwrap();
        assert!(!store.book_exists(&hash, &paths).await.unwrap());
        assert_eq!(failures.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn dropping_download_writer_removes_its_temporary_file() {
        let library = tempfile::tempdir().unwrap();
        let store = super::super::AssetStore::open(library.path().to_str().unwrap()).unwrap();
        let destination = library.path().join("book.epub");
        std::fs::write(&destination, b"existing book").unwrap();
        let hash = BookId::from(blake3::hash(b"existing book"));
        let path = RelativeBookPath::parse("book.epub").unwrap();
        let writer = store.begin_book_write(&path, &hash).unwrap();
        assert_eq!(std::fs::read_dir(library.path()).unwrap().count(), 3);
        drop(writer);
        assert_eq!(std::fs::read(&destination).unwrap(), b"existing book");
        assert_eq!(std::fs::read_dir(library.path()).unwrap().count(), 2);
    }

    #[test]
    fn cancelled_thumbnail_write_preserves_artwork_and_releases_its_lease() {
        let library = tempfile::tempdir().unwrap();
        let handle = open(library.path().to_str().unwrap()).unwrap();
        let hash = blake3::hash(b"thumbnail owner").to_hex().to_string();
        let destination = handle.assets_root.join("thumbnail").join(&hash);
        std::fs::write(&destination, b"original cover").unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().max_blocking_threads(1).build().unwrap();
        let (release, released) = std::sync::mpsc::channel();
        let blocker = runtime.spawn_blocking(move || released.recv_timeout(Duration::from_secs(5)).unwrap());
        runtime.block_on(async {
            let mut pending = Box::pin(write(&handle, format!("thumbnail/{hash}"), b"replacement cover"));
            assert!(futures_util::poll!(pending.as_mut()).is_pending());
            assert!(lease(&handle, &hash, true).unwrap().is_none());
            drop(pending);
            assert!(lease(&handle, &hash, true).unwrap().is_some());
            release.send(()).unwrap();
            blocker.await.unwrap();
        });
        // Wait for any abandoned writes to the temporary file to finish too.
        drop(runtime);
        assert_eq!(std::fs::read(&destination).unwrap(), b"original cover");
    }
}

#[cfg(test)]
mod prepared_file_tests {
    use super::*;

    #[test]
    fn prepared_removal_preserves_a_file_changed_before_publish() {
        let root = tempfile::tempdir().unwrap();
        let handle = open(root.path().to_str().unwrap()).unwrap();
        let path = root.path().join("book.epub");
        std::fs::write(&path, b"original").unwrap();
        let hash = BookId::from(blake3::hash(b"original"));
        let prepared = prepare_removal(&handle, &hash, &RelativeBookPath::parse("/book.epub").unwrap()).unwrap();
        std::fs::write(&path, b"externally changed book").unwrap();
        assert!(prepared.publish().is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"externally changed book");
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[tokio::test]
    async fn imported_and_renamed_m4b_uses_current_byte_identity() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("book.m4b");
        std::fs::write(&path, include_bytes!("../../../tests/fixtures/embedded-cover.m4b")).unwrap();
        let original_identity = BookId::from(hash_file(&mut File::open(&path).unwrap()).unwrap());
        // A non-audio MP4 edit changes identity because all file bytes are hashed.
        use std::io::Write as _;
        OpenOptions::new().append(true).open(&path).unwrap().write_all(b"\0\0\0\x0cfreedata").unwrap();
        assert!(!content_matches(&path, &original_identity).unwrap());
        let identity = BookId::from(hash_file(&mut File::open(&path).unwrap()).unwrap());
        assert!(content_matches(&path, &identity).unwrap());
        let store = super::super::AssetStore::open(directory.path().to_str().unwrap()).unwrap();
        let copied = store.stage_book(RelativeBookPath::parse("copy.m4b").unwrap(), Box::new(File::open(&path).unwrap())).await.unwrap();
        assert!(content_matches(copied.path(), &identity).unwrap());
        // Renaming preserves bytes and therefore preserves identity.
        let renamed = directory.path().join("extensionless");
        std::fs::rename(path, &renamed).unwrap();
        assert!(content_matches(&renamed, &identity).unwrap());
        assert!(!content_matches(&renamed, &BookId::new(&"0".repeat(64))).unwrap());
    }
}

pub(crate) fn create_child_directory(path: &Path) -> Result<(), Error> {
    match std::fs::create_dir(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::AlreadyExists && path.is_dir() => Ok(()),
        Err(error) => Err(error),
    }
}

/// Files in library folders keep their names. Only identity-keyed proof and
/// thumbnail sidecars move; this is safe to repeat after a partial migration.
pub(crate) fn migrate_book_ids(handle: &Handle, ids: &[(BookId, BookId)]) -> Result<(), AssetStoreError> {
    for &(old, new) in ids {
        let proof = handle.assets_root.join("local-proofs").join(old.to_hex());
        for checksum in read_local_proof(&proof)?.lines() {
            let checksum = checksum.parse::<ContentHash>().map_err(AssetStoreError::operation)?;
            remember_local_checksum(handle, &new, checksum)?;
        }
        match std::fs::remove_file(&proof) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let directory = handle.assets_root.join("thumbnail");
        let old = old.to_hex();
        for file in std::fs::read_dir(&directory)? {
            let file = file?;
            let name = file.file_name();
            let Some(suffix) = name.to_str().and_then(|name| name.strip_prefix(&old)).filter(|suffix| suffix.is_empty() || suffix.starts_with('.')) else { continue };
            let destination = directory.join(format!("{new}{suffix}"));
            if destination.try_exists()? {
                std::fs::remove_file(file.path())?;
            } else {
                std::fs::rename(file.path(), destination)?;
            }
        }
    }
    Ok(())
}
