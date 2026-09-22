//! Filesystem-only inventory discovery.
//!
//! The session supplies an immutable database snapshot. Inspection retries
//! unstable files locally; the session owns scan scheduling and commits.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use futures_util::StreamExt;

use book_model::BookFormat;
use library_database::{FilesystemScan, InspectedFilesystemScan, ScanDirectoryObservation, ScanFileObservation, ScanSnapshot, ScannedBook};
use sync_common::{ContentHash, DirId, ROOT_DIR_ID};

pub(crate) const PUBLISH_BATCH_SIZE: usize = 32;
const MAX_CONCURRENT_INSPECTIONS: usize = 8;

fn excluded(snapshot: &ScanSnapshot, path: &Path) -> bool {
    snapshot.excluded_paths.iter().any(|excluded| {
        let excluded = Path::new(excluded);
        path == excluded || path.starts_with(excluded)
    })
}

fn supported(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| BookFormat::from_extension(&extension.to_ascii_lowercase()).is_some())
}

/// Walk `root` and return directory/file observations. Marker creation is
/// physical filesystem work; all inventory decisions are deferred to the caller.
pub fn discover(root: impl Into<PathBuf>, snapshot: &ScanSnapshot) -> FilesystemScan {
    let root = root.into();
    let known = snapshot.directories.iter().map(|directory| (directory.id, directory)).collect::<HashMap<_, _>>();
    let mut result = FilesystemScan { readable: true, ..FilesystemScan::default() };
    let mut walk = library_files::filesystem::walk(&root);
    let mut parents = vec![ROOT_DIR_ID];
    let mut seen = HashSet::from([ROOT_DIR_ID]);

    while let Some(entry) = walk.next() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                log::warn!("scanner skipped unreadable filesystem entry: {error}");
                result.readable = false;
                continue;
            }
        };
        if entry.file_name().to_string_lossy().starts_with('.') {
            if entry.is_dir() {
                walk.skip_current_dir();
            }
            continue;
        }
        let relative = match entry.path().strip_prefix(&root) {
            Ok(path) => path,
            Err(_) => {
                result.readable = false;
                if entry.is_dir() {
                    walk.skip_current_dir();
                }
                continue;
            }
        };
        if excluded(snapshot, relative) {
            if entry.is_dir() {
                walk.skip_current_dir();
            }
            continue;
        }
        while parents.len() > entry.depth() {
            parents.pop();
        }
        let Some(parent_id) = parents.last().copied() else {
            result.readable = false;
            continue;
        };
        if entry.is_dir() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                result.readable = false;
                walk.skip_current_dir();
                continue;
            };
            let marker = entry.path().join(".biblos_uuid");
            let mut id = match library_files::filesystem::read_to_string(&marker) {
                Ok(marker) => match DirId::parse_str(marker.trim()) {
                    Ok(id) => id,
                    Err(_) => {
                        result.readable = false;
                        walk.skip_current_dir();
                        continue;
                    }
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    let id = DirId::new_v4();
                    if library_files::filesystem::write(&marker, id.to_string()).is_err() {
                        result.readable = false;
                        walk.skip_current_dir();
                        continue;
                    }
                    id
                }
                Err(_) => {
                    result.readable = false;
                    walk.skip_current_dir();
                    continue;
                }
            };
            if known.get(&id).is_some_and(|known| known.deleted) {
                parents.push(id);
                continue;
            }
            if !seen.insert(id) {
                id = DirId::new_v4();
                if library_files::filesystem::write(&marker, id.to_string()).is_err() {
                    result.readable = false;
                    walk.skip_current_dir();
                    continue;
                }
                seen.insert(id);
            }
            let Some(path) = relative.to_str().map(str::to_owned) else {
                result.readable = false;
                walk.skip_current_dir();
                continue;
            };
            result.directories.push(ScanDirectoryObservation { id, parent_id, name, path });
            parents.push(id);
        } else if entry.is_file() && supported(entry.path()) {
            let (Some(name), Some(path)) = (entry.file_name().to_str(), relative.to_str()) else {
                result.readable = false;
                continue;
            };
            match library_files::filesystem::fingerprint(entry.path()) {
                Ok(fingerprint) => result.files.push(ScanFileObservation { directory: parent_id, name: name.to_owned(), path: path.to_owned(), fingerprint }),
                Err(_) => result.readable = false,
            }
        }
    }
    result
}

/// Content hash, fingerprint, checksum, size, and extension: everything
/// about a file that requires reading its bytes but not parsing its
/// structure. Run on a blocking-pool thread so a large book's full-content
/// hash never blocks the library's own actor thread.
fn hash_book(path: PathBuf) -> Result<(ContentHash, u64, ContentHash, u64, String), std::io::Error> {
    use std::io::Read as _;
    let fingerprint = library_files::filesystem::fingerprint(&path)?;
    let mut reader = library_files::filesystem::open(&path)?;
    // Detect from bytes, not the extension: PDF/audio identities are stable
    // across metadata edits, while other books use their byte checksum.
    let content_hash = match book_identity::detect(&mut reader)? {
        Some(_) => Some(book_identity::ensure(&path)?),
        None => None,
    };
    // PDF stamping intentionally changes the file. Restart from its new
    // fingerprint, just as for an external edit, before accepting any bytes.
    require_unchanged(&path, fingerprint)?;
    let mut hasher = blake3::Hasher::new();
    let mut bytes = [0_u8; 64 * 1024];
    let mut size_bytes = 0_u64;
    loop {
        let read = reader.read(&mut bytes)?;
        if read == 0 {
            break;
        }
        hasher.update(&bytes[..read]);
        size_bytes = size_bytes.saturating_add(read as u64);
    }
    if size_bytes == 0 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "empty book"));
    }
    let extension = path.extension().and_then(|extension| extension.to_str()).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "unsupported book"))?.to_ascii_lowercase();
    require_unchanged(&path, fingerprint)?;
    let checksum = ContentHash::new(hasher.finalize().to_hex().as_str());
    Ok((content_hash.unwrap_or(checksum), fingerprint, checksum, size_bytes, extension))
}

fn require_unchanged(path: &Path, fingerprint: u64) -> std::io::Result<()> {
    if library_files::filesystem::fingerprint(path)? != fingerprint {
        return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "book changed during inspection"));
    }
    Ok(())
}

fn retryable(error: &std::io::Error) -> bool {
    matches!(error.kind(), std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)
}

async fn inspect_file(path: PathBuf, file: &ScanFileObservation) -> std::io::Result<ScannedBook> {
    let hashed = crate::executor::run_blocking({
        let path = path.clone();
        move || hash_book(path)
    })
    .await
    .map_err(|error| std::io::Error::other(error.to_string()))??;
    let (content_hash, fingerprint, checksum, size_bytes, extension) = hashed;
    let format = BookFormat::from_extension(&extension).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "unsupported book format"))?;
    let reader = library_files::filesystem::open(&path)?;
    let inspection = crate::worker_cpu::submit_book(crate::worker_cpu::Inspect { source_name: file.name.clone(), format }, Box::new(reader) as crate::BoxedBookReader).await;
    // A parse failure during an edit is transient; a parse failure on stable
    // bytes is not. Check stability before classifying the worker result.
    require_unchanged(&path, fingerprint)?;
    let inspection = inspection.map_err(std::io::Error::other)?;
    Ok(ScannedBook { directory: file.directory, name: file.name.clone(), path: file.path.clone(), fingerprint, content_hash, checksum, size_bytes, extension, inspection, inspection_version: crate::inspection_version(format) })
}

/// Inspects new or changed files using the immutable inventory snapshot. Failed files
/// remain visible through `readable = false`; the session decides whether that
/// permits a placement cleanup pass. A book whose metadata cannot be
/// extracted is left out of this pass entirely (not committed title-less)
/// and picked up again by a later scan.
pub async fn inspect(root: impl Into<PathBuf>, scan: &FilesystemScan, snapshot: &ScanSnapshot) -> InspectedFilesystemScan {
    let root = root.into();
    let mut result = InspectedFilesystemScan { readable: scan.readable, ..InspectedFilesystemScan::default() };
    let known = snapshot.books.iter().map(|book| ((book.directory, book.name.as_str()), book)).collect::<HashMap<_, _>>();
    let candidates = scan.files.iter().filter(|file| {
        // Keep all observations for mark-seen, but only read and parse books
        // whose placement is new or whose bytes may have changed. Legacy
        // records without an inspection must still be populated once.
        let version = Path::new(&file.name).extension().and_then(|extension| extension.to_str()).and_then(|extension| BookFormat::from_extension(&extension.to_ascii_lowercase())).map(crate::inspection_version);
        !known.get(&(file.directory, file.name.as_str())).is_some_and(|book| book.fingerprint == file.fingerprint && !book.description_missing && version.is_some() && book.inspection_version == version)
    });
    let mut inspections = futures_util::stream::iter(candidates.map(|file| {
        let path = root.join(&file.path);
        async move {
            for attempt in 0..3 {
                match inspect_file(path.clone(), file).await {
                    Ok(book) => return Ok(book),
                    Err(error) if attempt == 2 || !retryable(&error) => {
                        log::warn!("scanner skipped {} after {} inspection attempts: {error}", path.display(), attempt + 1);
                        return Err((file.path.clone(), error.to_string()));
                    }
                    Err(_) => crate::executor::sleep(std::time::Duration::from_millis(100)).await,
                }
            }
            unreachable!("inspection attempts return a result")
        }
    }))
    .buffered(MAX_CONCURRENT_INSPECTIONS);
    while let Some(inspection) = inspections.next().await {
        match inspection {
            Ok(book) => result.books.push(book),
            Err(failure) => {
                result.readable = false;
                result.failures.push(failure);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_byte_identity_reuses_checksum() {
        let root = tempfile::tempdir().unwrap();
        // Larger than the hash buffer, to exercise multiple reads.
        let bytes = vec![42; 150_000];
        let path = root.path().join("book.epub");
        std::fs::write(&path, &bytes).unwrap();
        let (identity, _, checksum, size, _) = hash_book(path).unwrap();
        assert_eq!(identity, checksum);
        assert_eq!(checksum, ContentHash::new(blake3::hash(&bytes).to_hex().as_str()));
        assert_eq!(size, bytes.len() as u64);
    }

    #[test]
    fn scan_retries_only_transient_errors() {
        use std::io::{Error, ErrorKind::*};
        for kind in [Interrupted, WouldBlock, TimedOut] {
            assert!(retryable(&Error::from(kind)));
        }
        for kind in [InvalidData, PermissionDenied, NotFound, Other] {
            assert!(!retryable(&Error::from(kind)));
        }
    }

    #[tokio::test]
    async fn scan_stable_invalid_metadata_is_not_retryable() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("book.epub");
        std::fs::write(&path, b"not an epub").unwrap();
        let file = ScanFileObservation { directory: ROOT_DIR_ID, name: "book.epub".into(), path: "book.epub".into(), fingerprint: 0 };
        let error = inspect_file(path, &file).await.unwrap_err();
        assert!(!retryable(&error));
    }

    #[test]
    fn scan_stability_rejects_replacement_after_hashing() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("book.epub");
        std::fs::write(&path, b"original bytes").unwrap();
        let (_, fingerprint, _, _, _) = hash_book(path.clone()).unwrap();
        require_unchanged(&path, fingerprint).unwrap();
        let replacement = root.path().join("replacement");
        std::fs::write(&replacement, b"different replacement bytes").unwrap();
        std::fs::rename(replacement, &path).unwrap();
        assert_eq!(require_unchanged(&path, fingerprint).unwrap_err().kind(), std::io::ErrorKind::Interrupted);
        let (_, fresh, _, _, _) = hash_book(path.clone()).unwrap();
        require_unchanged(&path, fresh).unwrap();
    }

    #[test]
    fn scan_pdf_stamping_is_accepted_on_fresh_attempt() {
        use lopdf::{dictionary, Document, Object};
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("book.pdf");
        let mut doc = Document::with_version("1.7");
        let pages = doc.new_object_id();
        let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(), 0.into(), 600.into(), 800.into()] });
        doc.objects.insert(pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }));
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        doc.trailer.set("Root", catalog);
        doc.save(&path).unwrap();
        assert_eq!(hash_book(path.clone()).unwrap_err().kind(), std::io::ErrorKind::Interrupted);
        let (identity, fingerprint, checksum, _, _) = hash_book(path.clone()).unwrap();
        assert_eq!(identity, book_identity::ensure(&path).unwrap());
        assert_ne!(identity, checksum, "stamped bytes differ from the original identity");
        require_unchanged(&path, fingerprint).unwrap();
        let renamed = root.path().join("misnamed.epub");
        std::fs::rename(&path, &renamed).unwrap();
        assert_eq!(hash_book(renamed).unwrap().0, identity, "identity detection must not rely on the extension");
    }

    #[tokio::test]
    async fn fingerprints_skip_unchanged_files_but_not_changed_or_uninspected_files() {
        let root = tempfile::tempdir().unwrap();
        // Deliberately absent: a matching fingerprint must skip opening it.
        let scan = FilesystemScan { files: vec![ScanFileObservation { directory: ROOT_DIR_ID, name: "book.epub".into(), path: "book.epub".into(), fingerprint: 42 }], readable: true, ..Default::default() };
        let mut snapshot = ScanSnapshot {
            placements: Vec::new(),
            work_revision: 0,
            scan_id: 1,
            excluded_paths: vec![],
            directories: vec![],
            tombstoned_files: vec![],
            audible_current: vec![],
            books: vec![library_database::ScanKnownBook {
                content_hash: ContentHash::new(&"a".repeat(64)),
                directory: ROOT_DIR_ID,
                name: "book.epub".into(),
                fingerprint: 42,
                description_missing: false,
                inspection_version: Some(crate::inspection_version(BookFormat::Epub)),
            }],
        };
        let unchanged = inspect(root.path(), &scan, &snapshot).await;
        assert!(unchanged.readable);
        assert!(unchanged.books.is_empty());
        assert_eq!(scan.files.len(), 1, "unchanged files remain available for mark-seen");

        snapshot.books[0].fingerprint = 41;
        assert!(!inspect(root.path(), &scan, &snapshot).await.readable);
        snapshot.books[0].fingerprint = 42;
        snapshot.books[0].description_missing = true;
        assert!(!inspect(root.path(), &scan, &snapshot).await.readable);
        snapshot.books[0].description_missing = false;
        snapshot.books[0].inspection_version = None;
        assert!(!inspect(root.path(), &scan, &snapshot).await.readable);
        snapshot.books[0].inspection_version = Some(crate::inspection_version(BookFormat::Epub) - 1);
        assert!(!inspect(root.path(), &scan, &snapshot).await.readable);
        snapshot.books[0].inspection_version = Some(crate::inspection_version(BookFormat::Epub));
        snapshot.books[0].name = "old-name.epub".into();
        assert!(!inspect(root.path(), &scan, &snapshot).await.readable, "new placements require inspection");
    }
}
