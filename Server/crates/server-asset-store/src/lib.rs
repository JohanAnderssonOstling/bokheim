use async_trait::async_trait;
use server_core::{AssetError, AssetKind, AssetReader, AssetStore, StoredAsset, SweepReport};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use sync_common::ContentHash;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, SeekFrom};
use tokio::sync::Semaphore;

const MAX_THUMBNAIL_DIMENSION: u32 = 4096;
const MAX_THUMBNAIL_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_CONCURRENT_BOOK_VALIDATIONS: usize = 2;

pub struct FsAssetStore {
    books_root: PathBuf,
    thumbnails_root: PathBuf,
    book_validation_permits: Arc<Semaphore>,
}

impl FsAssetStore {
    pub fn new(books_root: PathBuf, thumbnails_root: PathBuf) -> Self {
        Self { books_root, thumbnails_root, book_validation_permits: Arc::new(Semaphore::new(MAX_CONCURRENT_BOOK_VALIDATIONS)) }
    }

    fn path(&self, kind: AssetKind, hash: &ContentHash) -> PathBuf {
        match kind {
            AssetKind::Book => self.books_root.join(&hash.as_str()[..2]).join(hash.as_str()),
            AssetKind::Thumbnail => self.thumbnails_root.join(format!("{hash}.png")),
        }
    }

    fn root(&self, kind: AssetKind) -> &Path {
        match kind {
            AssetKind::Book => &self.books_root,
            AssetKind::Thumbnail => &self.thumbnails_root,
        }
    }

    async fn consider_file(path: &Path, live: &HashSet<ContentHash>, cutoff: SystemTime) -> SweepReport {
        let Ok(metadata) = tokio::fs::metadata(path).await else {
            return SweepReport::default();
        };
        if !metadata.is_file() {
            return SweepReport::default();
        }
        let is_temp = path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.contains(".tmp-"));
        let stem = path.file_stem().and_then(|stem| stem.to_str()).unwrap_or("");
        if !is_temp && ContentHash::parse(stem).is_ok_and(|hash| live.contains(&hash)) {
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

#[async_trait]
impl AssetStore for FsAssetStore {
    async fn initialize(&self) -> Result<(), AssetError> {
        tokio::fs::create_dir_all(&self.books_root).await.map_err(|error| AssetError::Storage(error.to_string()))?;
        tokio::fs::create_dir_all(&self.thumbnails_root).await.map_err(|error| AssetError::Storage(error.to_string()))?;
        Ok(())
    }

    async fn exists(&self, kind: AssetKind, hash: &ContentHash) -> Result<bool, AssetError> {
        match tokio::fs::metadata(self.path(kind, hash)).await {
            Ok(metadata) => Ok(metadata.is_file()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(AssetError::Storage(error.to_string())),
        }
    }

    async fn existing(&self, kind: AssetKind, hashes: Vec<ContentHash>) -> Result<Vec<ContentHash>, AssetError> {
        let mut present = Vec::new();
        for hash in hashes {
            if self.exists(kind, &hash).await? {
                present.push(hash);
            }
        }
        Ok(present)
    }

    async fn size(&self, kind: AssetKind, hash: &ContentHash) -> Result<Option<u64>, AssetError> {
        match tokio::fs::metadata(self.path(kind, hash)).await {
            Ok(metadata) if metadata.is_file() => Ok(Some(metadata.len())),
            Ok(_) => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(AssetError::Storage(error.to_string())),
        }
    }

    async fn open_from(&self, kind: AssetKind, hash: &ContentHash, offset: u64) -> Result<Option<StoredAsset>, AssetError> {
        let mut file = match File::open(self.path(kind, hash)).await {
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

    async fn put(&self, kind: AssetKind, hash: &ContentHash, expected_len: Option<u64>, mut reader: AssetReader) -> Result<u64, AssetError> {
        let final_path = self.path(kind, hash);
        let parent = final_path.parent().ok_or_else(|| AssetError::Storage("asset path has no parent".to_string()))?;
        tokio::fs::create_dir_all(parent).await.map_err(|error| AssetError::Storage(error.to_string()))?;
        let temp_path = PathBuf::from(format!("{}.tmp-{}", final_path.display(), uuid::Uuid::new_v4()));
        let mut file = File::create(&temp_path).await.map_err(|error| AssetError::Storage(error.to_string()))?;
        let mut hasher = (kind == AssetKind::Book).then(blake3::Hasher::new);
        let mut thumbnail_bytes = (kind == AssetKind::Thumbnail).then(Vec::new);
        let mut total = 0_u64;
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
            if let Some(hasher) = hasher.as_mut() {
                hasher.update(&buffer[..count]);
            }
            if let Some(bytes) = thumbnail_bytes.as_mut() {
                bytes.extend_from_slice(&buffer[..count]);
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
        if let Err(error) = file.flush().await {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(AssetError::Storage(error.to_string()));
        }
        if let Err(error) = file.sync_all().await {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(AssetError::Storage(error.to_string()));
        }
        drop(file);
        if hasher.is_some_and(|hasher| hasher.finalize().to_hex().as_str() != hash.as_str()) {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(AssetError::HashMismatch);
        }
        if kind == AssetKind::Book {
            let permit = match self.book_validation_permits.clone().acquire_owned().await {
                Ok(permit) => permit,
                Err(error) => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    return Err(AssetError::Storage(format!("book validation queue closed: {error}")));
                }
            };
            let validation_path = temp_path.clone();
            match tokio::task::spawn_blocking(move || {
                let _permit = permit;
                book_format::validate_path(validation_path)
            })
            .await
            {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    return Err(AssetError::InvalidInput(format!("book validation failed: {error}")));
                }
                Err(error) => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    return Err(AssetError::Storage(format!("book validation task failed: {error}")));
                }
            }
        }
        if let Some(bytes) = thumbnail_bytes {
            let image = match image::load_from_memory_with_format(&bytes, image::ImageFormat::Png) {
                Ok(image) => image,
                Err(error) => {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    return Err(AssetError::InvalidInput(format!("thumbnail must be a valid PNG: {error}")));
                }
            };
            let (width, height) = (image.width(), image.height());
            if width == 0
                || height == 0
                || width > MAX_THUMBNAIL_DIMENSION
                || height > MAX_THUMBNAIL_DIMENSION
                || u64::from(width) * u64::from(height) > MAX_THUMBNAIL_PIXELS
            {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(AssetError::InvalidInput(format!("thumbnail dimensions {width}x{height} exceed limits")));
            }
        }

        // Publish immutably: hard-link creation is atomic and fails rather
        // than overwriting an already published content-addressed object.
        match tokio::fs::hard_link(&temp_path, &final_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let _ = tokio::fs::remove_file(&temp_path).await;
                let existing_len = tokio::fs::metadata(&final_path).await.map_err(|metadata_error| AssetError::Storage(metadata_error.to_string()))?.len();
                if existing_len != total {
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
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| AssetError::Storage(error.to_string()))?;
        Ok(total)
    }

    async fn sweep(&self, kind: AssetKind, live: &HashSet<ContentHash>, cutoff: SystemTime) -> Result<SweepReport, AssetError> {
        let mut report = SweepReport::default();
        let mut top = match tokio::fs::read_dir(self.root(kind)).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(report),
            Err(error) => return Err(AssetError::Storage(error.to_string())),
        };
        while let Some(entry) = top.next_entry().await.map_err(|error| AssetError::Storage(error.to_string()))? {
            let path = entry.path();
            if kind == AssetKind::Book {
                if !path.is_dir() {
                    continue;
                }
                let mut inner = match tokio::fs::read_dir(path).await {
                    Ok(entries) => entries,
                    Err(_) => continue,
                };
                while let Ok(Some(file)) = inner.next_entry().await {
                    let item = Self::consider_file(&file.path(), live, cutoff).await;
                    report.scanned += item.scanned;
                    report.deleted += item.deleted;
                    report.bytes_reclaimed += item.bytes_reclaimed;
                }
            } else {
                let item = Self::consider_file(&path, live, cutoff).await;
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

    fn png(pixel: [u8; 4]) -> Vec<u8> {
        let mut image = DynamicImage::new_rgba8(1, 1).to_rgba8();
        image.put_pixel(0, 0, image::Rgba(pixel));
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image).write_to(&mut bytes, ImageOutputFormat::Png).unwrap();
        bytes.into_inner()
    }

    fn epub(undeclared_image: bool) -> Vec<u8> {
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let compressed = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        archive.start_file("mimetype", stored).unwrap();
        archive.write_all(b"application/epub+zip").unwrap();
        archive.start_file("META-INF/container.xml", compressed).unwrap();
        archive.write_all(br#"<?xml version="1.0"?><container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        archive.start_file("OEBPS/content.opf", compressed).unwrap();
        archive.write_all(br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata/><manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="chapter"/></spine></package>"#).unwrap();
        archive.start_file("OEBPS/chapter.xhtml", compressed).unwrap();
        archive.write_all(br#"<html xmlns="http://www.w3.org/1999/xhtml"><body>book</body></html>"#).unwrap();
        if undeclared_image {
            archive.start_file("OEBPS/hidden.png", compressed).unwrap();
            archive.write_all(&png([1, 2, 3, 255])).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }

    #[tokio::test]
    async fn verifies_books_but_keys_thumbnails_by_source_content() {
        let temp = tempfile::tempdir().unwrap();
        let store = FsAssetStore::new(temp.path().join("books"), temp.path().join("thumbs"));
        store.initialize().await.unwrap();
        let bytes = epub(false);
        let len = bytes.len() as u64;
        let hash = ContentHash::parse(blake3::hash(&bytes).to_hex().as_str()).unwrap();
        store.put(AssetKind::Book, &hash, Some(len), reader(bytes)).await.unwrap();
        assert!(store.exists(AssetKind::Book, &hash).await.unwrap());
        assert!(matches!(store.put(AssetKind::Book, &hash, Some(5), reader(b"wrong".to_vec())).await, Err(AssetError::HashMismatch)));
        let mut stored = store.open_from(AssetKind::Book, &hash, 0).await.unwrap().unwrap();
        let mut stored_bytes = Vec::new();
        stored.reader.read_to_end(&mut stored_bytes).await.unwrap();
        assert_eq!(stored_bytes, epub(false));
        store.put(AssetKind::Thumbnail, &hash, None, reader(png([1, 2, 3, 255]))).await.unwrap();
        assert!(store.exists(AssetKind::Thumbnail, &hash).await.unwrap());
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
        store.put(AssetKind::Book, &hash, Some(expected_len), reader(bytes)).await.unwrap();

        let mut stored = store.open_from(AssetKind::Book, &hash, 4).await.unwrap().unwrap();
        let mut suffix = Vec::new();
        stored.reader.read_to_end(&mut suffix).await.unwrap();
        assert_eq!((stored.offset, stored.total_len, suffix), (4, expected_len, expected_suffix));
        assert!(matches!(store.open_from(AssetKind::Book, &hash, expected_len).await, Err(AssetError::RangeNotSatisfiable { .. })));
    }

    #[tokio::test]
    async fn rejects_disguised_and_manifest_undeclared_book_content_before_publication() {
        let temp = tempfile::tempdir().unwrap();
        let store = FsAssetStore::new(temp.path().join("books"), temp.path().join("thumbs"));
        store.initialize().await.unwrap();
        for bytes in [png([1, 2, 3, 255]), epub(true)] {
            let len = bytes.len() as u64;
            let hash = ContentHash::parse(blake3::hash(&bytes).to_hex().as_str()).unwrap();
            assert!(matches!(store.put(AssetKind::Book, &hash, Some(len), reader(bytes)).await, Err(AssetError::InvalidInput(_))));
            assert!(!store.exists(AssetKind::Book, &hash).await.unwrap());
        }
    }

    #[tokio::test]
    async fn declared_book_length_is_enforced_before_publication() {
        let temp = tempfile::tempdir().unwrap();
        let store = FsAssetStore::new(temp.path().join("books"), temp.path().join("thumbs"));
        store.initialize().await.unwrap();
        let bytes = epub(false);
        let hash = ContentHash::parse(blake3::hash(&bytes).to_hex().as_str()).unwrap();
        assert!(matches!(
            store.put(AssetKind::Book, &hash, Some(bytes.len() as u64 - 1), reader(bytes)).await,
            Err(AssetError::LengthMismatch { .. })
        ));
        assert!(!store.exists(AssetKind::Book, &hash).await.unwrap());
    }

    #[tokio::test]
    async fn thumbnail_validation_limits_and_immutable_publication_are_enforced() {
        let temp = tempfile::tempdir().unwrap();
        let store = FsAssetStore::new(temp.path().join("books"), temp.path().join("thumbs"));
        store.initialize().await.unwrap();
        let hash = ContentHash::parse(&"a".repeat(64)).unwrap();
        assert!(matches!(store.put(AssetKind::Thumbnail, &hash, None, reader(b"not png".to_vec())).await, Err(AssetError::InvalidInput(_))));
        assert!(!store.exists(AssetKind::Thumbnail, &hash).await.unwrap());
        assert!(matches!(
            store.put(AssetKind::Thumbnail, &hash, None, reader(vec![0; AssetKind::Thumbnail.max_bytes() as usize + 1])).await,
            Err(AssetError::TooLarge { .. })
        ));

        let first = png([1, 2, 3, 255]);
        store.put(AssetKind::Thumbnail, &hash, None, reader(first.clone())).await.unwrap();
        store.put(AssetKind::Thumbnail, &hash, None, reader(png([9, 8, 7, 255]))).await.unwrap();
        let mut stored = store.open_from(AssetKind::Thumbnail, &hash, 0).await.unwrap().unwrap();
        let mut actual = Vec::new();
        stored.reader.read_to_end(&mut actual).await.unwrap();
        assert_eq!(actual, first, "a later writer must not replace an immutable asset");
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

        let report = store.sweep(AssetKind::Book, &HashSet::new(), SystemTime::now() + std::time::Duration::from_secs(1)).await.unwrap();
        assert_eq!((report.scanned, report.deleted), (1, 1));
        assert!(!orphan.exists());
    }
}
