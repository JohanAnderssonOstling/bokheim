use server_core::{AssetError, AssetKind, AssetReader, LiveAssets, StoredAsset, SweepReport};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::SystemTime;
use sync_common::ContentHash;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const DEFAULT_MAX_STAGED_BOOKS: usize = 16;
const DEFAULT_MAX_STAGED_BOOKS_PER_ACCOUNT: usize = 4;
const DEFAULT_MAX_STAGED_BYTES: u64 = 32 * 1024 * 1024 * 1024;
const DEFAULT_MAX_STAGED_BYTES_PER_ACCOUNT: u64 = 1_000_000_000;
const STAGING_BYTE_QUANTUM: u64 = 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UploadLimits {
    pub max_staged_books: usize,
    pub max_staged_books_per_account: usize,
    pub max_staged_bytes: u64,
    pub max_staged_bytes_per_account: u64,
}

impl Default for UploadLimits {
    fn default() -> Self {
        Self {
            max_staged_books: DEFAULT_MAX_STAGED_BOOKS,
            max_staged_books_per_account: DEFAULT_MAX_STAGED_BOOKS_PER_ACCOUNT,
            max_staged_bytes: DEFAULT_MAX_STAGED_BYTES,
            max_staged_bytes_per_account: DEFAULT_MAX_STAGED_BYTES_PER_ACCOUNT,
        }
    }
}

impl UploadLimits {
    pub fn from_env() -> Result<Self, String> {
        let names = [
            "BOKHEIM_MAX_STAGED_BOOK_UPLOADS",
            "BOKHEIM_MAX_STAGED_BOOK_UPLOADS_PER_ACCOUNT",
            "BOKHEIM_MAX_STAGED_BOOK_BYTES",
            "BOKHEIM_MAX_STAGED_BOOK_BYTES_PER_ACCOUNT",
        ];
        let mut values = HashMap::new();
        for name in names {
            match std::env::var(name) {
                Ok(value) => {
                    values.insert(name, value);
                }
                Err(std::env::VarError::NotPresent) => {}
                Err(error) => return Err(format!("cannot read {name}: {error}")),
            }
        }
        Self::from_values(|name| values.get(name).cloned())
    }

    fn from_values(mut value: impl FnMut(&str) -> Option<String>) -> Result<Self, String> {
        let defaults = Self::default();
        let integer = |raw: Option<String>, name: &str| raw.map(|raw| raw.parse::<u64>().map_err(|_| format!("{name} must be an integer"))).transpose();
        let max_staged_books = integer(value("BOKHEIM_MAX_STAGED_BOOK_UPLOADS"), "BOKHEIM_MAX_STAGED_BOOK_UPLOADS")?.unwrap_or(defaults.max_staged_books as u64);
        let max_staged_books_per_account = integer(value("BOKHEIM_MAX_STAGED_BOOK_UPLOADS_PER_ACCOUNT"), "BOKHEIM_MAX_STAGED_BOOK_UPLOADS_PER_ACCOUNT")?.unwrap_or(defaults.max_staged_books_per_account as u64);
        let max_staged_bytes = integer(value("BOKHEIM_MAX_STAGED_BOOK_BYTES"), "BOKHEIM_MAX_STAGED_BOOK_BYTES")?.unwrap_or(defaults.max_staged_bytes);
        let max_staged_bytes_per_account = integer(value("BOKHEIM_MAX_STAGED_BOOK_BYTES_PER_ACCOUNT"), "BOKHEIM_MAX_STAGED_BOOK_BYTES_PER_ACCOUNT")?.unwrap_or(defaults.max_staged_bytes_per_account);
        if !(1..=256).contains(&max_staged_books) || !(1..=max_staged_books).contains(&max_staged_books_per_account) {
            return Err("staged book upload counts must be between 1 and 256 and the per-account limit cannot exceed the global limit".to_string());
        }
        if !(STAGING_BYTE_QUANTUM..=1024 * 1024 * 1024 * 1024).contains(&max_staged_bytes) || !(STAGING_BYTE_QUANTUM..=max_staged_bytes).contains(&max_staged_bytes_per_account) {
            return Err("staged book byte limits must be between 1 MiB and 1 TiB and the per-account limit cannot exceed the global limit".to_string());
        }
        Ok(Self {
            max_staged_books: max_staged_books as usize,
            max_staged_books_per_account: max_staged_books_per_account as usize,
            max_staged_bytes,
            max_staged_bytes_per_account,
        })
    }
}

struct AccountStagingLimit {
    books: Arc<Semaphore>,
    bytes: Arc<Semaphore>,
}

struct StagingPermit {
    _account_books: OwnedSemaphorePermit,
    _account_bytes: OwnedSemaphorePermit,
    _global_books: OwnedSemaphorePermit,
    _global_bytes: OwnedSemaphorePermit,
    _account: Arc<AccountStagingLimit>,
}

struct StagingGate {
    books: Arc<Semaphore>,
    bytes: Arc<Semaphore>,
    per_account_books: usize,
    per_account_byte_units: usize,
    accounts: Mutex<HashMap<String, Weak<AccountStagingLimit>>>,
}

impl StagingGate {
    fn new(limits: UploadLimits) -> Self {
        Self {
            books: Arc::new(Semaphore::new(limits.max_staged_books)),
            bytes: Arc::new(Semaphore::new((limits.max_staged_bytes / STAGING_BYTE_QUANTUM) as usize)),
            per_account_books: limits.max_staged_books_per_account,
            per_account_byte_units: (limits.max_staged_bytes_per_account / STAGING_BYTE_QUANTUM) as usize,
            accounts: Mutex::new(HashMap::new()),
        }
    }

    fn acquire(&self, user_id: &str, size_bytes: u64) -> Result<StagingPermit, AssetError> {
        let units = size_bytes.div_ceil(STAGING_BYTE_QUANTUM).max(1) as u32;
        let account = {
            let mut accounts = self.accounts.lock().map_err(|_| AssetError::Storage("upload staging account lock is poisoned".to_string()))?;
            accounts.retain(|_, account| account.strong_count() > 0);
            match accounts.get(user_id).and_then(Weak::upgrade) {
                Some(account) => account,
                None => {
                    let account = Arc::new(AccountStagingLimit { books: Arc::new(Semaphore::new(self.per_account_books)), bytes: Arc::new(Semaphore::new(self.per_account_byte_units)) });
                    accounts.insert(user_id.to_string(), Arc::downgrade(&account));
                    account
                }
            }
        };
        let account_books = account.books.clone().try_acquire_owned().map_err(|_| AssetError::Busy)?;
        let account_bytes = account.bytes.clone().try_acquire_many_owned(units).map_err(|_| AssetError::Busy)?;
        let global_books = self.books.clone().try_acquire_owned().map_err(|_| AssetError::Busy)?;
        let global_bytes = self.bytes.clone().try_acquire_many_owned(units).map_err(|_| AssetError::Busy)?;
        Ok(StagingPermit { _account_books: account_books, _account_bytes: account_bytes, _global_books: global_books, _global_bytes: global_bytes, _account: account })
    }
}

pub struct FsAssetStore {
    books_root: PathBuf,
    thumbnails_root: PathBuf,
    staging: StagingGate,
}

impl FsAssetStore {
    pub fn new(books_root: PathBuf, thumbnails_root: PathBuf) -> Self {
        Self::with_upload_limits(books_root, thumbnails_root, UploadLimits::default())
    }

    pub fn with_upload_limits(books_root: PathBuf, thumbnails_root: PathBuf, limits: UploadLimits) -> Self {
        Self { books_root, thumbnails_root, staging: StagingGate::new(limits) }
    }

    fn user_scope(user_id: &str) -> String {
        blake3::hash(user_id.as_bytes()).to_hex().to_string()
    }

    fn path(&self, user_id: &str, kind: AssetKind, hash: &ContentHash) -> PathBuf {
        match kind {
            AssetKind::Book => self.books_root.join(&hash.as_str()[..2]).join(hash.as_str()),
            AssetKind::Thumbnail => self.thumbnails_root.join("users").join(Self::user_scope(user_id)).join(format!("{hash}.jpg")),
        }
    }

    fn root(&self, kind: AssetKind) -> &Path {
        match kind {
            AssetKind::Book => &self.books_root,
            AssetKind::Thumbnail => &self.thumbnails_root,
        }
    }

    async fn probe_root(label: &str, root: &Path, create: bool) -> Result<(), AssetError> {
        if create {
            tokio::fs::create_dir_all(root).await.map_err(|error| AssetError::Storage(format!("cannot create {label} storage directory {}: {error}", root.display())))?;
        }
        let metadata = tokio::fs::metadata(root).await.map_err(|error| AssetError::Storage(format!("cannot inspect {label} storage directory {}: {error}", root.display())))?;
        if !metadata.is_dir() {
            return Err(AssetError::Storage(format!("{label} storage path is not a directory: {}", root.display())));
        }

        let probe = root.join(format!(".bokheim-write-check-{}", uuid::Uuid::new_v4()));
        let mut file = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&probe).await.map_err(|error| AssetError::Storage(format!("{label} storage directory is not writable at {}: {error}", root.display())))?;
        if let Err(error) = file.write_all(b"bokheim-storage-check").await {
            drop(file);
            let _ = tokio::fs::remove_file(&probe).await;
            return Err(AssetError::Storage(format!("cannot write to {label} storage directory {}: {error}", root.display())));
        }
        if let Err(error) = file.sync_all().await {
            drop(file);
            let _ = tokio::fs::remove_file(&probe).await;
            return Err(AssetError::Storage(format!("cannot sync {label} storage directory probe {}: {error}", root.display())));
        }
        drop(file);
        tokio::fs::remove_file(&probe).await.map_err(|error| AssetError::Storage(format!("cannot remove probe from {label} storage directory {}: {error}", root.display())))?;
        Ok(())
    }

    fn is_live(kind: AssetKind, root: &Path, path: &Path, hash: &ContentHash, live: &LiveAssets, scoped_thumbnails: &HashSet<(String, ContentHash)>) -> bool {
        match kind {
            AssetKind::Book => live.books().contains(hash),
            AssetKind::Thumbnail => {
                let components = path.strip_prefix(root).ok().map(|relative| relative.components().filter_map(|component| component.as_os_str().to_str()).collect::<Vec<_>>()).unwrap_or_default();
                if let ["users", scope, _file] = components.as_slice() {
                    scoped_thumbnails.contains(&(scope.to_string(), hash.clone()))
                } else {
                    // Compatibility files from the old global layouts remain
                    // live while any owner references the corresponding book.
                    live.thumbnails().iter().any(|thumbnail| thumbnail.content_hash == *hash)
                }
            }
        }
    }

    async fn consider_file(kind: AssetKind, root: &Path, path: &Path, live: &LiveAssets, scoped_thumbnails: &HashSet<(String, ContentHash)>, cutoff: SystemTime) -> SweepReport {
        let Ok(metadata) = tokio::fs::metadata(path).await else {
            return SweepReport::default();
        };
        if !metadata.is_file() {
            return SweepReport::default();
        }
        let is_temp = path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.contains(".tmp-"));
        let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("");
        if !is_temp && ContentHash::parse(stem).is_ok_and(|hash| Self::is_live(kind, root, path, &hash, live, scoped_thumbnails)) {
            return SweepReport { scanned: 1, ..SweepReport::default() };
        }
        if metadata.modified().map(|modified| modified > cutoff).unwrap_or(true) {
            return SweepReport { scanned: 1, ..SweepReport::default() };
        }
        if tokio::fs::remove_file(path).await.is_ok() {
            SweepReport { scanned: 1, deleted: 1, bytes_reclaimed: metadata.len() }
        } else {
            SweepReport { scanned: 1, ..SweepReport::default() }
        }
    }
}

impl FsAssetStore {
    pub async fn initialize(&self) -> Result<(), AssetError> {
        Self::probe_root("book", &self.books_root, true).await?;
        Self::probe_root("thumbnail", &self.thumbnails_root, true).await?;
        Ok(())
    }

    pub async fn check_ready(&self) -> Result<(), AssetError> {
        Self::probe_root("book", &self.books_root, false).await?;
        Self::probe_root("thumbnail", &self.thumbnails_root, false).await?;
        Ok(())
    }

    pub async fn size(&self, user_id: &str, kind: AssetKind, hash: &ContentHash) -> Result<Option<u64>, AssetError> {
        match tokio::fs::metadata(self.path(user_id, kind, hash)).await {
            Ok(metadata) if metadata.is_file() => Ok(Some(metadata.len())),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(AssetError::Storage(error.to_string())),
        }
    }

    pub async fn open_from(&self, user_id: &str, kind: AssetKind, hash: &ContentHash, offset: u64) -> Result<Option<StoredAsset>, AssetError> {
        let mut file = match File::open(self.path(user_id, kind, hash)).await {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(AssetError::Storage(error.to_string())),
        };
        let len = file.metadata().await.map_err(|error| AssetError::Storage(error.to_string()))?.len();
        if offset > len || (offset > 0 && offset == len) {
            return Err(AssetError::RangeNotSatisfiable { offset, len });
        }
        file.seek(SeekFrom::Start(offset)).await.map_err(|error| AssetError::Storage(error.to_string()))?;
        Ok(Some(StoredAsset { reader: Box::new(file), total_len: len, offset }))
    }

    pub async fn put(&self, user_id: &str, kind: AssetKind, hash: &ContentHash, expected_len: Option<u64>, mut reader: AssetReader) -> Result<u64, AssetError> {
        let _staging_permit = if kind == AssetKind::Book {
            Some(self.staging.acquire(user_id, expected_len.ok_or(AssetError::LengthRequired)?)?)
        } else {
            None
        };
        let final_path = self.path(user_id, kind, hash);
        let parent = final_path.parent().ok_or_else(|| AssetError::Storage("asset path has no parent".to_string()))?;
        tokio::fs::create_dir_all(parent).await.map_err(|error| AssetError::Storage(error.to_string()))?;
        let temp_path = PathBuf::from(format!("{}.tmp-{}", final_path.display(), uuid::Uuid::new_v4()));
        let mut file = File::create(&temp_path).await.map_err(|error| AssetError::Storage(error.to_string()))?;
        let mut total = 0_u64;
        let mut book_hasher = (kind == AssetKind::Book).then(blake3::Hasher::new);
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let count = match reader.read(&mut buffer).await {
                Ok(count) => count,
                Err(error) => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    return Err(AssetError::InvalidInput(error.to_string()));
                }
            };
            if count == 0 {
                break;
            }
            total = total.saturating_add(count as u64);
            if let Some(hasher) = book_hasher.as_mut() {
                hasher.update(&buffer[..count]);
            }
            if expected_len.is_some_and(|expected| total > expected) {
                drop(file);
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(AssetError::LengthMismatch { expected: expected_len.unwrap_or(0), actual: total });
            }
            if total > kind.max_bytes() {
                drop(file);
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(AssetError::TooLarge { limit: kind.max_bytes() });
            }
            if let Err(error) = file.write_all(&buffer[..count]).await {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(AssetError::Storage(error.to_string()));
            }
        }
        if expected_len.is_some_and(|expected| total != expected) {
            drop(file);
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(AssetError::LengthMismatch { expected: expected_len.unwrap_or(0), actual: total });
        }
        if let Some(hasher) = book_hasher {
            if hasher.finalize().to_hex().as_str() != hash.as_str() {
                drop(file);
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(AssetError::HashMismatch);
            }
        }
        if let Err(error) = file.flush().await {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(AssetError::Storage(error.to_string()));
        }
        drop(file);
        let durable_file = match tokio::fs::OpenOptions::new().write(true).open(&temp_path).await {
            Ok(file) => file,
            Err(error) => {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(AssetError::Storage(error.to_string()));
            }
        };
        if let Err(error) = durable_file.sync_all().await {
            drop(durable_file);
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(AssetError::Storage(error.to_string()));
        }
        drop(durable_file);
        // Publish immutably: hard-link creation is atomic and fails rather
        // than overwriting an already published content-addressed object.
        match tokio::fs::hard_link(&temp_path, &final_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = tokio::fs::remove_file(&temp_path).await;
                let existing_len = tokio::fs::metadata(&final_path).await.map_err(|metadata_error| AssetError::Storage(metadata_error.to_string()))?.len();
                if kind == AssetKind::Book && existing_len != total {
                    return Err(AssetError::Storage(format!("existing content-addressed asset has length {existing_len}, uploaded copy has length {total}")));
                }
                return Ok(existing_len);
            }
            Err(error) => {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(AssetError::Storage(error.to_string()));
            }
        }
        if let Err(error) = tokio::fs::remove_file(&temp_path).await {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(AssetError::Storage(error.to_string()));
        }
        std::fs::File::open(parent).and_then(|directory| directory.sync_all()).map_err(|error| AssetError::Storage(error.to_string()))?;
        Ok(total)
    }

    pub async fn sweep(&self, kind: AssetKind, live: &LiveAssets, cutoff: SystemTime) -> Result<SweepReport, AssetError> {
        let mut report = SweepReport::default();
        let root = self.root(kind);
        let scoped_thumbnails = live.thumbnails().iter().map(|thumbnail| (Self::user_scope(&thumbnail.user_id), thumbnail.content_hash.clone())).collect::<HashSet<_>>();
        let mut pending = vec![root.to_path_buf()];
        while let Some(directory) = pending.pop() {
            let mut entries = match tokio::fs::read_dir(&directory).await {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound && directory == root => return Ok(report),
                Err(_) => continue,
            };
            while let Some(entry) = entries.next_entry().await.map_err(|error| AssetError::Storage(error.to_string()))? {
                let path = entry.path();
                if entry.file_type().await.map_err(|error| AssetError::Storage(error.to_string()))?.is_dir() {
                    pending.push(path);
                    continue;
                }
                let item = Self::consider_file(kind, root, &path, live, &scoped_thumbnails, cutoff).await;
                report.scanned += item.scanned;
                report.deleted += item.deleted;
                report.bytes_reclaimed += item.bytes_reclaimed;
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageOutputFormat};
    use std::io::{Cursor, Write};
    use tokio::io::{AsyncRead, AsyncReadExt};
    use zip::write::SimpleFileOptions;

    fn reader(bytes: Vec<u8>) -> Box<dyn AsyncRead + Unpin + Send> {
        Box::new(std::io::Cursor::new(bytes))
    }

    #[test]
    fn upload_capacity_is_bounded() {
        let configured = UploadLimits::from_values(|name| match name {
            "BOKHEIM_MAX_STAGED_BOOK_UPLOADS" => Some("8".to_string()),
            "BOKHEIM_MAX_STAGED_BOOK_UPLOADS_PER_ACCOUNT" => Some("2".to_string()),
            "BOKHEIM_MAX_STAGED_BOOK_BYTES" => Some((8 * STAGING_BYTE_QUANTUM).to_string()),
            "BOKHEIM_MAX_STAGED_BOOK_BYTES_PER_ACCOUNT" => Some((2 * STAGING_BYTE_QUANTUM).to_string()),
            _ => None,
        })
        .unwrap();
        assert_eq!(configured.max_staged_books, 8);
        assert_eq!(configured.max_staged_books_per_account, 2);
        assert_eq!(configured.max_staged_bytes, 8 * STAGING_BYTE_QUANTUM);
        assert_eq!(configured.max_staged_bytes_per_account, 2 * STAGING_BYTE_QUANTUM);
        assert!(UploadLimits::from_values(|name| (name == "BOKHEIM_MAX_STAGED_BOOK_UPLOADS_PER_ACCOUNT").then(|| "17".to_string())).is_err());
    }

    #[test]
    fn staged_upload_capacity_is_bounded_per_account_and_globally_and_releases_on_drop() {
        let limits = UploadLimits { max_staged_books: 2, max_staged_books_per_account: 1, max_staged_bytes: 2 * STAGING_BYTE_QUANTUM, max_staged_bytes_per_account: STAGING_BYTE_QUANTUM, ..UploadLimits::default() };
        let gate = StagingGate::new(limits);
        let alice = gate.acquire("alice", 1).unwrap();
        assert!(matches!(gate.acquire("alice", 1), Err(AssetError::Busy)));
        let bob = gate.acquire("bob", STAGING_BYTE_QUANTUM).unwrap();
        assert!(matches!(gate.acquire("carol", 1), Err(AssetError::Busy)));
        drop(alice);
        let carol = gate.acquire("carol", 1).unwrap();
        drop((bob, carol));
        assert!(gate.acquire("alice", STAGING_BYTE_QUANTUM).is_ok());
    }

    fn jpeg(pixel: [u8; 3]) -> Vec<u8> {
        let mut image = DynamicImage::new_rgb8(1, 1).to_rgb8();
        image.put_pixel(0, 0, image::Rgb(pixel));
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image).write_to(&mut bytes, ImageOutputFormat::Jpeg(75)).unwrap();
        bytes.into_inner()
    }

    fn epub_with_chapter(chapter: &str, undeclared_image: bool) -> Vec<u8> {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let compressed = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        archive.start_file("mimetype", stored).unwrap();
        archive.write_all(b"application/epub+zip").unwrap();
        archive.start_file("META-INF/container.xml", compressed).unwrap();
        archive
            .write_all(br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#)
            .unwrap();
        archive.start_file("OEBPS/content.opf", compressed).unwrap();
        archive.write_all(br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#).unwrap();
        archive.start_file("OEBPS/chapter.xhtml", compressed).unwrap();
        archive.write_all(format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>{chapter}</body></html>"#).as_bytes()).unwrap();
        if undeclared_image {
            archive.start_file("OEBPS/hidden.jpg", compressed).unwrap();
            archive.write_all(&jpeg([1, 2, 3])).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }

    fn epub(undeclared_image: bool) -> Vec<u8> {
        epub_with_chapter("book", undeclared_image)
    }

    #[tokio::test]
    async fn initialization_probes_storage_and_fails_for_a_non_directory_root() {
        let temp = tempfile::tempdir().unwrap();
        let books = temp.path().join("books");
        let thumbnails = temp.path().join("thumbnails");
        let store = FsAssetStore::new(books.clone(), thumbnails.clone());
        store.initialize().await.unwrap();
        assert_eq!(std::fs::read_dir(&books).unwrap().count(), 0, "write probe must be removed");
        assert_eq!(std::fs::read_dir(&thumbnails).unwrap().count(), 0, "write probe must be removed");

        let invalid = temp.path().join("not-a-directory");
        std::fs::write(&invalid, b"file").unwrap();
        let error = FsAssetStore::new(invalid.clone(), thumbnails).initialize().await.unwrap_err();
        assert!(error.to_string().contains("book storage"), "unexpected error: {error}");
        assert!(error.to_string().contains(&invalid.display().to_string()), "unexpected error: {error}");
    }

    #[tokio::test]
    async fn readiness_does_not_recreate_a_missing_storage_root() {
        let temp = tempfile::tempdir().unwrap();
        let books = temp.path().join("books");
        let thumbnails = temp.path().join("thumbs");
        let store = FsAssetStore::new(books.clone(), thumbnails);
        store.initialize().await.unwrap();
        tokio::fs::remove_dir(&books).await.unwrap();

        let error = store.check_ready().await.unwrap_err();
        assert!(error.to_string().contains("cannot inspect book storage"));
        assert!(!books.exists(), "readiness must not hide a lost mount by recreating its directory");
    }

    #[tokio::test]
    async fn verifies_books_but_keys_thumbnails_by_source_content() {
        let temp = tempfile::tempdir().unwrap();
        let store = FsAssetStore::new(temp.path().join("books"), temp.path().join("thumbs"));
        store.initialize().await.unwrap();
        let bytes = epub(false);
        let len = bytes.len() as u64;
        let hash = ContentHash::parse(blake3::hash(&bytes).to_hex().as_str()).unwrap();
        store.put("user", AssetKind::Book, &hash, Some(len), reader(bytes)).await.unwrap();
        assert!(store.size("other-user", AssetKind::Book, &hash).await.unwrap().is_some(), "book blobs remain globally deduplicated");
        let wrong = epub_with_chapter("different book", false);
        assert!(matches!(store.put("user", AssetKind::Book, &hash, Some(wrong.len() as u64), reader(wrong)).await, Err(AssetError::HashMismatch)));
        let mut stored = store.open_from("other-user", AssetKind::Book, &hash, 0).await.unwrap().unwrap();
        let mut stored_bytes = Vec::new();
        stored.reader.read_to_end(&mut stored_bytes).await.unwrap();
        assert_eq!(stored_bytes, epub(false));
        store.put("user", AssetKind::Thumbnail, &hash, None, reader(jpeg([1, 2, 3]))).await.unwrap();
        assert!(store.size("user", AssetKind::Thumbnail, &hash).await.unwrap().is_some());
        assert!(store.size("other-user", AssetKind::Thumbnail, &hash).await.unwrap().is_none(), "covers must not cross account boundaries");
    }

    #[tokio::test]
    async fn ranges_report_total_length_and_stream_only_the_suffix() {
        let temp = tempfile::tempdir().unwrap();
        let store = FsAssetStore::new(temp.path().join("books"), temp.path().join("thumbs"));
        store.initialize().await.unwrap();
        let bytes = epub(false);
        let expected_suffix = bytes[4..].to_vec();
        let expected_len = bytes.len() as u64;
        let hash = ContentHash::parse(blake3::hash(&bytes).to_hex().as_str()).unwrap();
        store.put("user", AssetKind::Book, &hash, Some(expected_len), reader(bytes)).await.unwrap();

        let mut stored = store.open_from("user", AssetKind::Book, &hash, 4).await.unwrap().unwrap();
        let mut suffix = Vec::new();
        stored.reader.read_to_end(&mut suffix).await.unwrap();
        assert_eq!((stored.offset, stored.total_len, suffix), (4, expected_len, expected_suffix));
        assert!(matches!(store.open_from("user", AssetKind::Book, &hash, expected_len).await, Err(AssetError::RangeNotSatisfiable { .. })));
    }

    #[tokio::test]
    async fn declared_book_length_is_enforced_before_publication() {
        let temp = tempfile::tempdir().unwrap();
        let store = FsAssetStore::new(temp.path().join("books"), temp.path().join("thumbs"));
        store.initialize().await.unwrap();
        let bytes = epub(false);
        let hash = ContentHash::parse(blake3::hash(&bytes).to_hex().as_str()).unwrap();
        assert!(matches!(store.put("user", AssetKind::Book, &hash, Some(bytes.len() as u64 - 1), reader(bytes)).await, Err(AssetError::LengthMismatch { .. })));
        assert!(store.size("user", AssetKind::Book, &hash).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn thumbnails_are_opaque_bounded_and_isolated_by_user() {
        let temp = tempfile::tempdir().unwrap();
        let thumbnails = temp.path().join("thumbs");
        let store = FsAssetStore::new(temp.path().join("books"), thumbnails.clone());
        store.initialize().await.unwrap();
        let hash = ContentHash::parse(&"a".repeat(64)).unwrap();
        assert!(matches!(store.put("alice", AssetKind::Thumbnail, &hash, None, reader(vec![0; AssetKind::Thumbnail.max_bytes() as usize + 1])).await, Err(AssetError::TooLarge { .. })));

        let first = jpeg([1, 2, 3]);
        let second = jpeg([9, 8, 7]);
        store.put("alice", AssetKind::Thumbnail, &hash, None, reader(first.clone())).await.unwrap();
        store.put("alice", AssetKind::Thumbnail, &hash, None, reader(second.clone())).await.unwrap();
        store.put("bob", AssetKind::Thumbnail, &hash, None, reader(second.clone())).await.unwrap();

        for (user, expected) in [("alice", &first), ("bob", &second)] {
            let mut stored = store.open_from(user, AssetKind::Thumbnail, &hash, 0).await.unwrap().unwrap();
            let mut actual = Vec::new();
            stored.reader.read_to_end(&mut actual).await.unwrap();
            assert_eq!(&actual, expected, "each account gets its own original immutable bytes");
        }

        // The server intentionally does not inspect cover contents. Clients
        // validate and decode within their own limits before display.
        let opaque_hash = ContentHash::parse(&"b".repeat(64)).unwrap();
        let opaque = b"arbitrary private client bytes".to_vec();
        store.put("alice", AssetKind::Thumbnail, &opaque_hash, None, reader(opaque.clone())).await.unwrap();
        let mut stored = store.open_from("alice", AssetKind::Thumbnail, &opaque_hash, 0).await.unwrap().unwrap();
        let mut actual = Vec::new();
        stored.reader.read_to_end(&mut actual).await.unwrap();
        assert_eq!(actual, opaque);

        let live = LiveAssets::new(HashSet::new(), HashSet::from([server_core::UserThumbnail { user_id: "alice".to_owned(), content_hash: hash.clone() }]));
        let kept = store.sweep(AssetKind::Thumbnail, &live, SystemTime::now() + std::time::Duration::from_secs(1)).await.unwrap();
        assert_eq!((kept.scanned, kept.deleted), (3, 2), "Bob's cover and the unrelated cover are not kept alive by Alice");
        assert!(store.size("alice", AssetKind::Thumbnail, &hash).await.unwrap().is_some());
        assert!(store.size("bob", AssetKind::Thumbnail, &hash).await.unwrap().is_none());
        let deleted = store.sweep(AssetKind::Thumbnail, &LiveAssets::default(), SystemTime::now() + std::time::Duration::from_secs(1)).await.unwrap();
        assert_eq!((deleted.scanned, deleted.deleted), (1, 1));
    }

    #[tokio::test]
    async fn sweep_reclaims_abandoned_upload_temps() {
        let temp = tempfile::tempdir().unwrap();
        let books = temp.path().join("books");
        let store = FsAssetStore::new(books.clone(), temp.path().join("thumbs"));
        store.initialize().await.unwrap();
        let shard = books.join("aa");
        tokio::fs::create_dir_all(&shard).await.unwrap();
        let orphan = shard.join(format!("{}.tmp-{}", "a".repeat(64), uuid::Uuid::new_v4()));
        tokio::fs::write(&orphan, b"partial").await.unwrap();

        let report = store.sweep(AssetKind::Book, &LiveAssets::default(), SystemTime::now() + std::time::Duration::from_secs(1)).await.unwrap();
        assert_eq!((report.scanned, report.deleted), (1, 1));
        assert!(!orphan.exists());
    }
}
