use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use sync_common::ContentHash;
use tokio::fs::File;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub type AssetReader = Box<dyn AsyncRead + Unpin + Send>;
pub const MAX_BOOK_UPLOAD_DURATION: Duration = Duration::from_secs(2 * 60 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    Book,
    Thumbnail,
}

impl AssetKind {
    pub const fn max_bytes(self) -> Option<u64> {
        match self {
            Self::Book => None,
            Self::Thumbnail => Some(sync_common::MAX_THUMBNAIL_BYTES),
        }
    }
}

pub struct StoredAsset {
    pub reader: AssetReader,
    pub total_len: u64,
    pub offset: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum AssetError {
    #[error("asset access is forbidden")]
    Forbidden,
    #[error("invalid asset input: {0}")]
    InvalidInput(String),
    #[error("asset bytes do not match the requested content hash")]
    HashMismatch,
    #[error("book uploads require an exact Content-Length")]
    LengthRequired,
    #[error("asset body length {actual} does not match the declared length {expected}")]
    LengthMismatch { expected: u64, actual: u64 },
    #[error("an upload for this user and book is already in progress")]
    UploadInProgress,
    #[error("book upload staging capacity is temporarily exhausted")]
    Busy,
    #[error("book storage quota exceeded: limit={limit}, used={used}, reserved={reserved}, requested={requested}")]
    QuotaExceeded { limit: u64, used: u64, reserved: u64, requested: u64 },
    #[error("asset exceeds the {limit}-byte limit")]
    TooLarge { limit: u64 },
    #[error("asset byte offset {offset} is beyond its {len}-byte length")]
    RangeNotSatisfiable { offset: u64, len: u64 },
    #[error("asset storage failed: {0}")]
    Storage(String),
}

const DEFAULT_MAX_STAGED_BOOKS: usize = 16;
const DEFAULT_MAX_STAGED_BOOKS_PER_ACCOUNT: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UploadLimits {
    pub max_staged_books: usize,
    pub max_staged_books_per_account: usize,
}

impl Default for UploadLimits {
    fn default() -> Self {
        Self { max_staged_books: DEFAULT_MAX_STAGED_BOOKS, max_staged_books_per_account: DEFAULT_MAX_STAGED_BOOKS_PER_ACCOUNT }
    }
}

impl UploadLimits {
    pub fn from_env() -> Result<Self, String> {
        let names = ["BOKHEIM_MAX_STAGED_BOOK_UPLOADS", "BOKHEIM_MAX_STAGED_BOOK_UPLOADS_PER_ACCOUNT"];
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
        if !(1..=256).contains(&max_staged_books) || !(1..=max_staged_books).contains(&max_staged_books_per_account) {
            return Err("staged book upload counts must be between 1 and 256 and the per-account limit cannot exceed the global limit".to_string());
        }
        Ok(Self { max_staged_books: max_staged_books as usize, max_staged_books_per_account: max_staged_books_per_account as usize })
    }
}

struct AccountStagingLimit {
    books: Arc<Semaphore>,
}

struct StagingPermit {
    _account_books: OwnedSemaphorePermit,
    _global_books: OwnedSemaphorePermit,
    _account: Arc<AccountStagingLimit>,
}

struct StagingGate {
    books: Arc<Semaphore>,
    per_account_books: usize,
    accounts: Mutex<HashMap<String, Weak<AccountStagingLimit>>>,
}

impl StagingGate {
    fn new(limits: UploadLimits) -> Self {
        Self { books: Arc::new(Semaphore::new(limits.max_staged_books)), per_account_books: limits.max_staged_books_per_account, accounts: Mutex::new(HashMap::new()) }
    }

    fn acquire(&self, user_id: &str, _size_bytes: u64) -> Result<StagingPermit, AssetError> {
        let account = {
            let mut accounts = self.accounts.lock().map_err(|_| AssetError::Storage("upload staging account lock is poisoned".to_string()))?;
            accounts.retain(|_, account| account.strong_count() > 0);
            match accounts.get(user_id).and_then(Weak::upgrade) {
                Some(account) => account,
                None => {
                    let account = Arc::new(AccountStagingLimit { books: Arc::new(Semaphore::new(self.per_account_books)) });
                    accounts.insert(user_id.to_string(), Arc::downgrade(&account));
                    account
                }
            }
        };
        let account_books = account.books.clone().try_acquire_owned().map_err(|_| AssetError::Busy)?;
        let global_books = self.books.clone().try_acquire_owned().map_err(|_| AssetError::Busy)?;
        Ok(StagingPermit { _account_books: account_books, _global_books: global_books, _account: account })
    }
}

pub struct FsAssetStore {
    books_root: PathBuf,
    thumbnails_root: PathBuf,
    staging: StagingGate,
}

struct TemporaryFile {
    path: Option<PathBuf>,
}

impl TemporaryFile {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn path(&self) -> &Path {
        self.path.as_deref().expect("temporary file path is present until cleanup")
    }

    fn remove(mut self) -> std::io::Result<()> {
        let path = self.path.take().expect("temporary file path is present until cleanup");
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                self.path = Some(path);
                Err(error)
            }
        }
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if let Some(path) = self.path.take() {
            let _ = std::fs::remove_file(path);
        }
    }
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

    fn path(&self, user_id: &str, kind: AssetKind, hash: &ContentHash) -> Result<PathBuf, AssetError> {
        Ok(match kind {
            AssetKind::Book => self.books_root.join(&hash.as_str()[..2]).join(hash.as_str()),
            AssetKind::Thumbnail => self.thumbnails_root.join("users").join(Self::user_scope(user_id)).join(format!("{hash}.jpg")),
        })
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
        match tokio::fs::metadata(self.path(user_id, kind, hash)?).await {
            Ok(metadata) if metadata.is_file() => Ok(Some(metadata.len())),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(AssetError::Storage(error.to_string())),
        }
    }

    pub async fn open_from(&self, user_id: &str, kind: AssetKind, hash: &ContentHash, offset: u64) -> Result<Option<StoredAsset>, AssetError> {
        let mut file = match File::open(self.path(user_id, kind, hash)?).await {
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
        let _staging_permit = if kind == AssetKind::Book { Some(self.staging.acquire(user_id, expected_len.ok_or(AssetError::LengthRequired)?)?) } else { None };
        let final_path = self.path(user_id, kind, hash)?;
        let parent = final_path.parent().ok_or_else(|| AssetError::Storage("asset path has no parent".to_string()))?;
        tokio::fs::create_dir_all(parent).await.map_err(|error| AssetError::Storage(error.to_string()))?;
        let temp = TemporaryFile::new(PathBuf::from(format!("{}.tmp-{}", final_path.display(), uuid::Uuid::new_v4())));
        // Open synchronously so cancellation cannot race an offloaded create
        // operation and strand a file after the cleanup guard has run.
        let mut file = File::from_std(std::fs::File::create(temp.path()).map_err(|error| AssetError::Storage(error.to_string()))?);
        let mut total = 0_u64;
        let mut book_hasher = (kind == AssetKind::Book).then(blake3::Hasher::new);
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            let count = match reader.read(&mut buffer).await {
                Ok(count) => count,
                Err(error) => return Err(AssetError::InvalidInput(error.to_string())),
            };
            if count == 0 {
                break;
            }
            total = total.saturating_add(count as u64);
            if let Some(hasher) = book_hasher.as_mut() {
                hasher.update(&buffer[..count]);
            }
            if expected_len.is_some_and(|expected| total > expected) {
                return Err(AssetError::LengthMismatch { expected: expected_len.unwrap_or(0), actual: total });
            }
            if kind.max_bytes().is_some_and(|limit| total > limit) {
                return Err(AssetError::TooLarge { limit: kind.max_bytes().unwrap_or(u64::MAX) });
            }
            file.write_all(&buffer[..count]).await.map_err(|error| AssetError::Storage(error.to_string()))?;
        }
        if expected_len.is_some_and(|expected| total != expected) {
            return Err(AssetError::LengthMismatch { expected: expected_len.unwrap_or(0), actual: total });
        }
        if let Some(hasher) = book_hasher {
            if hasher.finalize().to_hex().as_str() != hash.as_str() {
                return Err(AssetError::HashMismatch);
            }
        }
        file.flush().await.map_err(|error| AssetError::Storage(error.to_string()))?;
        drop(file);
        let durable_file = tokio::fs::OpenOptions::new().write(true).open(temp.path()).await.map_err(|error| AssetError::Storage(error.to_string()))?;
        durable_file.sync_all().await.map_err(|error| AssetError::Storage(error.to_string()))?;
        drop(durable_file);
        // Publish immutably: hard-link creation is atomic and fails rather
        // than overwriting an already published content-addressed object.
        match tokio::fs::hard_link(temp.path(), &final_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing_len = tokio::fs::metadata(&final_path).await.map_err(|metadata_error| AssetError::Storage(metadata_error.to_string()))?.len();
                if kind == AssetKind::Book && existing_len != total {
                    return Err(AssetError::Storage(format!("existing content-addressed asset has length {existing_len}, uploaded copy has length {total}")));
                }
                return Ok(existing_len);
            }
            Err(error) => return Err(AssetError::Storage(error.to_string())),
        }
        temp.remove().map_err(|error| AssetError::Storage(error.to_string()))?;
        std::fs::File::open(parent).and_then(|directory| directory.sync_all()).map_err(|error| AssetError::Storage(error.to_string()))?;
        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageOutputFormat};
    use std::io::{Cursor, Write};
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
    use zip::write::SimpleFileOptions;

    fn reader(bytes: Vec<u8>) -> Box<dyn AsyncRead + Unpin + Send> {
        Box::new(std::io::Cursor::new(bytes))
    }

    #[test]
    fn upload_capacity_is_bounded() {
        let configured = UploadLimits::from_values(|name| match name {
            "BOKHEIM_MAX_STAGED_BOOK_UPLOADS" => Some("8".to_string()),
            "BOKHEIM_MAX_STAGED_BOOK_UPLOADS_PER_ACCOUNT" => Some("2".to_string()),
            _ => None,
        })
        .unwrap();
        assert_eq!(configured.max_staged_books, 8);
        assert_eq!(configured.max_staged_books_per_account, 2);
        assert!(UploadLimits::from_values(|name| (name == "BOKHEIM_MAX_STAGED_BOOK_UPLOADS_PER_ACCOUNT").then(|| "17".to_string())).is_err());
    }

    #[test]
    fn staged_upload_capacity_is_bounded_per_account_and_globally_and_releases_on_drop() {
        let limits = UploadLimits { max_staged_books: 2, max_staged_books_per_account: 1 };
        let gate = StagingGate::new(limits);
        let alice = gate.acquire("alice", 1).unwrap();
        assert!(matches!(gate.acquire("alice", 1), Err(AssetError::Busy)));
        let bob = gate.acquire("bob", u64::MAX).unwrap();
        assert!(matches!(gate.acquire("carol", 1), Err(AssetError::Busy)));
        drop(alice);
        let carol = gate.acquire("carol", 1).unwrap();
        drop((bob, carol));
        assert!(gate.acquire("alice", u64::MAX).is_ok());
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
        let hash = ContentHash::new(&blake3::hash(&bytes).to_hex().as_str());
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
        let hash = ContentHash::new(&blake3::hash(&bytes).to_hex().as_str());
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
        let hash = ContentHash::new(&blake3::hash(&bytes).to_hex().as_str());
        assert!(matches!(store.put("user", AssetKind::Book, &hash, Some(bytes.len() as u64 - 1), reader(bytes)).await, Err(AssetError::LengthMismatch { .. })));
        assert!(store.size("user", AssetKind::Book, &hash).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn cancelling_an_upload_removes_its_temporary_file() {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(FsAssetStore::new(temp.path().join("books"), temp.path().join("thumbs")));
        store.initialize().await.unwrap();
        let hash = ContentHash::new(&"a".repeat(64));
        let final_path = store.path("user", AssetKind::Book, &hash).unwrap();
        let parent = final_path.parent().unwrap().to_path_buf();
        let (mut writer, reader) = tokio::io::duplex(8);
        writer.write_all(b"a").await.unwrap();

        let uploading = {
            let store = store.clone();
            tokio::spawn(async move { store.put("user", AssetKind::Book, &hash, Some(2), Box::new(reader)).await })
        };
        let mut temporary_file_created = false;
        for _ in 0..1_000 {
            if parent.exists() && std::fs::read_dir(&parent).unwrap().next().is_some() {
                temporary_file_created = true;
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(temporary_file_created, "the upload should create its temporary file before cancellation");
        uploading.abort();
        assert!(uploading.await.unwrap_err().is_cancelled());
        assert!(!final_path.exists());
        assert_eq!(std::fs::read_dir(parent).unwrap().count(), 0, "cancellation must not strand a temporary file");
    }

    #[tokio::test]
    async fn thumbnails_are_opaque_bounded_and_isolated_by_user() {
        let temp = tempfile::tempdir().unwrap();
        let thumbnails = temp.path().join("thumbs");
        let store = FsAssetStore::new(temp.path().join("books"), thumbnails.clone());
        store.initialize().await.unwrap();
        let hash = ContentHash::new(&"a".repeat(64));
        let thumbnail_limit = AssetKind::Thumbnail.max_bytes().unwrap();
        assert!(matches!(store.put("alice", AssetKind::Thumbnail, &hash, None, reader(vec![0; thumbnail_limit as usize + 1])).await, Err(AssetError::TooLarge { .. })));

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
        let opaque_hash = ContentHash::new(&"b".repeat(64));
        let opaque = b"arbitrary private client bytes".to_vec();
        store.put("alice", AssetKind::Thumbnail, &opaque_hash, None, reader(opaque.clone())).await.unwrap();
        let mut stored = store.open_from("alice", AssetKind::Thumbnail, &opaque_hash, 0).await.unwrap().unwrap();
        let mut actual = Vec::new();
        stored.reader.read_to_end(&mut actual).await.unwrap();
        assert_eq!(actual, opaque);
    }
}
