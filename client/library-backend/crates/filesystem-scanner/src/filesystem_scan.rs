//! Filesystem-only inventory discovery.
//!
//! The session supplies an immutable database snapshot. Inspection retries
//! unstable files locally; the session owns scan scheduling and commits.

pub use crate::discovery::is_audiobook_folder;
use crate::discovery::{audiobook_member, audiobook_tracks, next_book_entry};
use crate::ingestion::fingerprint as book_fingerprint;
use crate::ingestion::{READER_EPUB_METADATA_VERSION, READER_PDF_METADATA_VERSION, ingest_audiobook_folder};
use futures_util::StreamExt;
use library_files::asset_store::import_in_progress;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use book_model::BookFormat;
use library_database::{FilesystemScan, InspectedFilesystemScan, ScanDirectoryObservation, ScanFileObservation, ScanKnownBook, ScanSnapshot, ScannedBook};
use sync_common::{DirId, ROOT_DIR_ID};

pub const PUBLISH_BATCH_SIZE: usize = 32;
const MAX_CONCURRENT_INSPECTIONS: usize = 2;

/// Placement lookup for the immutable book inventory captured at scan startup.
/// Batch publication updates the snapshot's write guards, not its book inventory.
pub struct ScanBookIndex {
    by_placement: Vec<usize>,
}

impl ScanBookIndex {
    pub fn unchanged(&self, snapshot: &ScanSnapshot, file: &ScanFileObservation) -> bool {
        self.get(snapshot, file.directory, &file.name).is_some_and(|book| book.fingerprint == file.fingerprint)
    }

    pub fn new(snapshot: &ScanSnapshot) -> Self {
        let books = &snapshot.books;
        let mut by_placement: Vec<_> = (0..books.len()).collect();
        by_placement.sort_unstable_by(|&left, &right| (books[left].directory, books[left].name.as_str(), left).cmp(&(books[right].directory, books[right].name.as_str(), right)));
        Self { by_placement }
    }

    fn get<'a>(&self, snapshot: &'a ScanSnapshot, directory: DirId, name: &str) -> Option<&'a ScanKnownBook> {
        let end = self.by_placement.partition_point(|&index| {
            let book = &snapshot.books[index];
            (book.directory, book.name.as_str()) <= (directory, name)
        });
        // Keep the last inventory entry for duplicate placements, as the old
        // HashMap insertion order did.
        let index = *self.by_placement.get(end.checked_sub(1)?)?;
        let book = &snapshot.books[index];
        (book.directory == directory && book.name == name).then_some(book)
    }
}

fn excluded(snapshot: &ScanSnapshot, path: &Path) -> bool {
    snapshot.excluded_paths.iter().any(|excluded| {
        let excluded = Path::new(excluded);
        path == excluded || path.starts_with(excluded)
    })
}

fn excluded_audiobook_member(folder: &Path, excluded_paths: &[String]) -> bool {
    excluded_paths.iter().any(|excluded| {
        let excluded = Path::new(excluded);
        let Ok(relative) = excluded.strip_prefix(folder) else { return false };
        let Some(first) = relative.components().next() else { return true };
        let name = first.as_os_str().to_string_lossy();
        if audiobook_folder::is_disc_folder_name(&name) {
            return true;
        }
        if relative.components().count() != 1 {
            return false;
        }
        audiobook_member(Path::new(&*name))
    })
}

fn folder_fingerprint(folder: &Path, tracks: &[audiobook_archive::Track]) -> std::io::Result<u64> {
    library_files::filesystem::audiobook_fingerprint(folder, tracks)
}

// Database paths use forward slashes on every platform. Join native components
// rather than replacing characters that could be part of a Unix filename.
fn portable_relative_path(path: &Path) -> Option<String> {
    Some(path.iter().map(OsStr::to_str).collect::<Option<Vec<_>>>()?.join("/"))
}

/// Walk `root` and return directory/file observations. Marker creation is
/// physical filesystem work; all inventory decisions are deferred to the caller.
pub fn discover(root: impl Into<PathBuf>, snapshot: &ScanSnapshot) -> FilesystemScan {
    let root = root.into();
    let known = snapshot.directories.iter().map(|directory| (directory.id, directory)).collect::<HashMap<_, _>>();
    let tombstoned = snapshot.tombstoned_files.iter().map(|(directory, name)| (*directory, name.as_str())).collect::<HashSet<_>>();
    let mut result = FilesystemScan { readable: true, ..FilesystemScan::default() };
    let mut walk = library_files::filesystem::walk(&root);
    let mut parents = vec![ROOT_DIR_ID];
    let mut seen = HashSet::from([ROOT_DIR_ID]);

    while let Some(entry) = next_book_entry(&mut walk) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                log::warn!("scanner skipped unreadable filesystem entry: {error}");
                result.readable = false;
                continue;
            }
        };
        if import_in_progress(entry.path()) {
            // Android's compatibility copy may already have its final name.
            // Retry this scan without inspecting or retiring partial files.
            result.readable = false;
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
        let Some(relative_path) = portable_relative_path(relative) else {
            result.readable = false;
            if entry.is_dir() {
                walk.skip_current_dir();
            }
            continue;
        };
        while parents.len() > entry.depth() {
            parents.pop();
        }
        let Some(parent_id) = parents.last().copied() else {
            result.readable = false;
            continue;
        };
        // Soft removal retains the bytes on disk. Respect the saved placement
        // tombstone so a later scan cannot import the book back out of Trash.
        // Skip audiobook folders before walking their tracks as well.
        if entry.file_name().to_str().is_some_and(|name| tombstoned.contains(&(parent_id, name))) {
            if entry.is_dir() {
                walk.skip_current_dir();
            }
            continue;
        }
        if entry.is_dir() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                result.readable = false;
                walk.skip_current_dir();
                continue;
            };
            let marker = entry.path().join(".biblos_uuid");
            if library_files::filesystem::read_to_string(&marker).ok().and_then(|marker| DirId::parse_str(marker.trim()).ok()).is_some_and(|id| known.get(&id).is_some_and(|known| known.deleted)) {
                walk.skip_current_dir();
                continue;
            }
            match audiobook_tracks(entry.path(), &root, |path| excluded(snapshot, path)) {
                Ok(Some(_)) if excluded_audiobook_member(relative, &snapshot.excluded_paths) => {
                    walk.skip_current_dir();
                    continue;
                }
                Ok(Some(tracks)) => {
                    match folder_fingerprint(entry.path(), &tracks) {
                        Ok(fingerprint) => result.files.push(ScanFileObservation { directory: parent_id, name, path: relative_path, fingerprint }),
                        Err(_) => result.readable = false,
                    }
                    walk.skip_current_dir();
                    continue;
                }
                Ok(None) => {}
                Err(error) => {
                    log::warn!("scanner could not inspect audiobook folder {}: {error}", entry.path().display());
                    result.readable = false;
                    walk.skip_current_dir();
                    continue;
                }
            }
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
                // A tombstoned directory is not a valid import destination.
                // Do not inspect its descendants as books in that directory.
                walk.skip_current_dir();
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
            result.directories.push(ScanDirectoryObservation { id, parent_id, name, path: relative_path });
            parents.push(id);
        } else {
            let Some(name) = entry.file_name().to_str() else {
                result.readable = false;
                continue;
            };
            match book_fingerprint(entry.path()) {
                Ok(fingerprint) => result.files.push(ScanFileObservation { directory: parent_id, name: name.to_owned(), path: relative_path, fingerprint }),
                Err(_) => result.readable = false,
            }
        }
    }
    result
}

fn retryable(error: &std::io::Error) -> bool {
    matches!(error.kind(), std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut)
}

/// Inspects new or changed files using the immutable inventory snapshot. Failed files
/// remain visible through `readable = false`; the session decides whether that
/// permits a placement cleanup pass. A book whose metadata cannot be
/// extracted is left out of this pass entirely (not committed title-less)
/// and picked up again by a later scan.
pub async fn inspect(root: impl Into<PathBuf>, scan: &FilesystemScan, snapshot: &ScanSnapshot, host: &Arc<dyn cpu_host::CpuHost>) -> InspectedFilesystemScan {
    let known = ScanBookIndex::new(snapshot);
    inspect_files(root, &scan.files, scan.readable, snapshot, &known, host).await
}

/// Inspect a borrowed batch using the lookup shared by the complete scan.
pub async fn inspect_files(root: impl Into<PathBuf>, files: &[ScanFileObservation], readable: bool, snapshot: &ScanSnapshot, known: &ScanBookIndex, host: &Arc<dyn cpu_host::CpuHost>) -> InspectedFilesystemScan {
    let root = root.into();
    let mut result = InspectedFilesystemScan { readable, ..InspectedFilesystemScan::default() };
    let candidates: Vec<_> = files
        .iter()
        .filter(|file| {
            // Keep all observations for mark-seen, but only read and parse books
            // whose placement is new or whose bytes may have changed. Verified
            // audiobooks can reuse complete synced playback facts. Reader-library
            // PDFs retain their supplied metadata and defer reader data to opening.
            // Other legacy records without an inspection must be populated once.
            let format = if root.join(&file.path).is_dir() { Some(BookFormat::Mp3Folder) } else { BookFormat::from_file_name(&file.name) };
            let version = format.map(book_inspection::inspection_version);
            !known.get(snapshot, file.directory, &file.name).is_some_and(|book| {
                book.fingerprint == file.fingerprint
                    && version.is_some()
                    && (book.inspection_version == version
                        || (format == Some(BookFormat::Pdf) && book.inspection_version == Some(READER_PDF_METADATA_VERSION))
                        || (format == Some(BookFormat::Epub) && book.inspection_version == Some(READER_EPUB_METADATA_VERSION))
                        || (book.inspection_version.is_none() && book.cached_audiobook_format.is_some() && book.cached_audiobook_format == format))
            })
        })
        .collect();
    let inspections: Vec<_> = candidates
        .into_iter()
        .map(|file: &ScanFileObservation| {
            let path = root.join(&file.path);
            let root = root.clone();
            let host = host.clone();
            let cached_pdf = known.get(snapshot, file.directory, &file.name).and_then(|book| book.cached_pdf.clone());
            async move {
                for attempt in 0..3 {
                    match (async {
                        if import_in_progress(&path) {
                            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "book publication is still being prepared"));
                        }
                        let ingested = if path.is_dir() {
                            if excluded_audiobook_member(path.strip_prefix(&root).unwrap_or(&path), &snapshot.excluded_paths) {
                                return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "audiobook contains a temporarily excluded file"));
                            }
                            ingest_audiobook_folder(path.clone(), file.name.clone()).await?
                        } else {
                            crate::ingestion::ingest_file_with_pdf_reader(path.clone(), file.name.clone(), &*host, cached_pdf.as_deref().cloned()).await?
                        };
                        let crate::ingestion::IngestedFile { source_identity, audiobook_content_hash, byte_checksum, book_id, size_bytes, fingerprint, format, inspection } = ingested;
                        let extension = format.canonical_extension().to_owned();
                        Ok(ScannedBook {
                            source_identity,
                            audiobook_content_hash,
                            byte_checksum,
                            directory: file.directory,
                            name: file.name.clone(),
                            path: file.path.clone(),
                            fingerprint,
                            book_id,
                            size_bytes,
                            extension,
                            inspection,
                            inspection_version: book_inspection::inspection_version(format),
                        })
                    })
                    .await
                    {
                        Ok(book) => return Ok(book),
                        Err(error) if attempt == 2 || !retryable(&error) => {
                            log::warn!("scanner skipped {} after {} inspection attempts: {error}", path.display(), attempt + 1);
                            return Err((file.path.clone(), error.to_string()));
                        }
                        Err(_) => client_platform_runtime::executor::sleep(std::time::Duration::from_millis(100)).await.map_err(|error| (file.path.clone(), error.to_string()))?,
                    }
                }
                unreachable!("inspection attempts return a result")
            }
        })
        .collect();
    let mut inspections = futures_util::stream::iter(inspections).buffered(MAX_CONCURRENT_INSPECTIONS);
    while let Some(inspection) = inspections.next().await {
        match inspection {
            Ok(book) => result.books.push(book),
            Err((path, error)) => {
                log::warn!("could not inspect {path}: {error}");
                result.readable = false;
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use sync_common::BookId;

    /// Inspection dispatch is unreachable here: fixtures exercise selection and
    /// failure plumbing with the book file absent, so hashing fails first.
    struct FailingHost;
    impl cpu_host::CpuHost for FailingHost {
        fn book(&self, _task: cpu_host::BookTask, _reader: cpu_host::BoxedCpuReader) -> client_platform_runtime::executor::BoxedBackendFuture<'_, Result<cpu_host::CpuResponse, client_protocol::BackendError>> {
            Box::pin(async { Err("no CPU host in scanner tests".into()) })
        }
        fn request(&self, _request: cpu_host::CpuRequest) -> client_platform_runtime::executor::BoxedBackendFuture<'_, Result<cpu_host::CpuResponse, client_protocol::BackendError>> {
            Box::pin(async { Err("no CPU host in scanner tests".into()) })
        }
        fn generate_thumbnail(&self, _extension: String, _reader: cpu_host::BoxedCpuReader) -> client_platform_runtime::executor::BoxedBackendFuture<'_, Result<Option<cpu_host::ThumbnailBytes>, client_protocol::BackendError>> {
            Box::pin(async { Err("no CPU host in scanner tests".into()) })
        }
    }
    fn host() -> Arc<dyn cpu_host::CpuHost> {
        Arc::new(FailingHost)
    }

    #[tokio::test]
    async fn trashed_books_stay_trashed_after_reopening_and_scanning() {
        use book_inspection::InspectedBook;
        use book_model::{BookDetails, BookRecord};
        use library_database::{Database, ScanApplyResult};

        for format in [BookFormat::Epub, BookFormat::Mp3Folder] {
            for remove_last_placement in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let name = if format == BookFormat::Epub { "book.epub" } else { "Story" };
                let path = root.path().join(name);
                if format == BookFormat::Mp3Folder {
                    std::fs::create_dir(&path).unwrap();
                    std::fs::write(path.join("01.mp3"), b"audio").unwrap();
                } else {
                    std::fs::write(&path, b"book").unwrap();
                }
                let database_path = root.path().join(".library.db");
                let db = Database::open(&database_path).unwrap();
                db.initialize_library().await.unwrap();
                let mut snapshot = db.scan_snapshot(book_inspection::inspection_version(book_model::BookFormat::Pdf)).await.unwrap();
                let discovery = discover(root.path(), &snapshot);
                let fingerprint = discovery.files[0].fingerprint;
                let hash = BookId::from(blake3::hash(b"book"));
                let book = ScannedBook {
                    source_identity: None,
                    audiobook_content_hash: None,
                    byte_checksum: None,
                    directory: ROOT_DIR_ID,
                    name: name.into(),
                    path: name.into(),
                    fingerprint,
                    book_id: hash,
                    size_bytes: 4,
                    extension: format.canonical_extension().into(),
                    inspection: InspectedBook {
                        metadata: BookRecord { metadata: BookDetails { title: "Story".into(), subtitle: None, contributors: vec![], book: Default::default() }, description: String::new() },
                        toc: vec![],
                        audiobook: None,
                        pdf: None,
                    },
                    inspection_version: book_inspection::inspection_version(format),
                };
                let inspected = InspectedFilesystemScan { readable: true, books: vec![book] };
                assert_eq!(db.apply_filesystem_scan_batch(&mut snapshot, &discovery, inspected, true, None).unwrap(), ScanApplyResult::Applied);
                let hash = db.resolve_scanned_content(hash.content_hash().unwrap(), None).await.unwrap();
                if remove_last_placement {
                    assert!(db.remove_book_placement(&hash, &ROOT_DIR_ID).unwrap());
                } else {
                    db.trash_book(&hash).await.unwrap();
                }
                drop(db);

                let reopened = Database::open(&database_path).unwrap();
                reopened.initialize_library().unwrap();
                let mut snapshot = reopened.scan_snapshot(book_inspection::inspection_version(book_model::BookFormat::Pdf)).unwrap();
                assert_eq!(snapshot.tombstoned_files, [(ROOT_DIR_ID, name.into())]);
                let discovery = discover(root.path(), &snapshot);
                assert!(discovery.readable);
                assert!(discovery.files.is_empty(), "retained files must not be reimported");
                let inspected = inspect(root.path(), &discovery, &snapshot, &host()).await;
                assert!(inspected.readable);
                assert_eq!(reopened.apply_filesystem_scan_batch(&mut snapshot, &discovery, inspected, true, None).unwrap(), ScanApplyResult::Applied);
                assert_eq!(reopened.library_trash().unwrap().books[0].book_id, hash);
                assert!(path.exists(), "Trash must retain the physical book");

                reopened.restore_book(&hash).unwrap();
                let snapshot = reopened.scan_snapshot(book_inspection::inspection_version(book_model::BookFormat::Pdf)).unwrap();
                assert!(snapshot.tombstoned_files.is_empty(), "explicit restore clears the scan tombstone");
                assert!(reopened.library_trash().unwrap().books.is_empty());
            }
        }
    }

    #[test]
    fn removed_placements_do_not_hide_books_in_other_directories() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("Removed")).unwrap();
        std::fs::create_dir(root.path().join("Live")).unwrap();
        for name in ["book.epub", "Removed/book.epub", "Live/book.epub"] {
            std::fs::write(root.path().join(name), b"book").unwrap();
        }
        let mut snapshot = ScanSnapshot { placements: vec![], work_revision: 0, scan_id: 1, excluded_paths: vec![], directories: vec![], tombstoned_files: vec![], books: vec![] };
        let initial = discover(root.path(), &snapshot);
        let removed = initial.directories.iter().find(|directory| directory.name == "Removed").unwrap().id;
        snapshot.tombstoned_files = vec![(ROOT_DIR_ID, "book.epub".into()), (removed, "book.epub".into())];
        let scan = discover(root.path(), &snapshot);
        assert!(scan.readable);
        assert_eq!(scan.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["Live/book.epub"]);
        assert_eq!(scan.directories.len(), 2);
    }

    #[test]
    fn nested_books_and_audiobooks_use_portable_observation_paths() {
        let root = tempfile::tempdir().unwrap();
        let shelf = root.path().join("Shelf");
        let nested = shelf.join("Nested");
        let audiobook = shelf.join("Story");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&audiobook).unwrap();
        std::fs::write(nested.join("book.epub"), b"book").unwrap();
        std::fs::write(audiobook.join("01.mp3"), b"audio").unwrap();
        let snapshot = ScanSnapshot { placements: vec![], work_revision: 0, scan_id: 1, excluded_paths: vec![], directories: vec![], tombstoned_files: vec![], books: vec![] };
        let found = discover(root.path(), &snapshot);
        assert!(found.readable);
        let mut files = found.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>();
        files.sort_unstable();
        assert_eq!(files, ["Shelf/Nested/book.epub", "Shelf/Story"]);
        let mut directories = found.directories.iter().map(|directory| directory.path.as_str()).collect::<Vec<_>>();
        directories.sort_unstable();
        assert_eq!(directories, ["Shelf", "Shelf/Nested"]);
        for path in files.into_iter().chain(directories) {
            assert!(root.path().join(path).exists(), "portable path must still open its native entry: {path}");
        }
    }

    #[test]
    fn staged_copies_are_invisible_until_published() {
        let root = tempfile::tempdir().unwrap();
        let staging = root.path().join(".bokheim-copy-abandoned");
        std::fs::create_dir_all(staging.join("contents/Nested")).unwrap();
        std::fs::write(staging.join("contents/Nested/book.epub"), b"partial copy").unwrap();
        let snapshot = ScanSnapshot { placements: vec![], work_revision: 0, scan_id: 1, excluded_paths: vec![], directories: vec![], tombstoned_files: vec![], books: vec![] };
        let found = discover(root.path(), &snapshot);
        assert!(found.readable);
        assert!(found.files.is_empty());
        assert!(found.directories.is_empty());
        assert!(!staging.join("contents/.biblos_uuid").exists());

        std::fs::write(staging.join("contents/Nested/book.epub"), b"complete copy").unwrap();
        std::fs::rename(staging.join("contents"), root.path().join("Books")).unwrap();
        let found = discover(root.path(), &snapshot);
        assert!(found.readable);
        assert_eq!(found.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["Books/Nested/book.epub"]);
        assert!(found.directories.iter().all(|directory| !directory.path.starts_with('.')));
    }

    #[test]
    fn regular_scan_groups_mp3_tracks_as_one_book() {
        let root = tempfile::tempdir().unwrap();
        let book = root.path().join("Story");
        std::fs::create_dir_all(book.join("Disc 1")).unwrap();
        std::fs::write(book.join("Disc 1/01 Opening.mp3"), b"first").unwrap();
        std::fs::write(book.join("Disc 1/02 Ending.mp3"), b"last").unwrap();
        std::fs::write(book.join("book.nfo"), b"Title: Original\n").unwrap();
        let snapshot = ScanSnapshot { placements: vec![], work_revision: 0, scan_id: 1, excluded_paths: vec![], directories: vec![], tombstoned_files: vec![], books: vec![] };
        let first = discover(root.path(), &snapshot);
        assert!(first.readable);
        assert!(first.directories.is_empty());
        assert_eq!(first.files.len(), 1);
        assert_eq!(first.files[0].name, "Story");
        assert_eq!(first.files[0].path, "Story");
        let old_fingerprint = first.files[0].fingerprint;
        std::fs::write(book.join("book.nfo"), b"Title: Changed Story\n").unwrap();
        let nfo_changed = discover(root.path(), &snapshot);
        assert_ne!(nfo_changed.files[0].fingerprint, old_fingerprint);
        std::fs::write(book.join("book.cue"), b"FILE \"Disc 1/01 Opening.mp3\" MP3\n").unwrap();
        let cue_changed = discover(root.path(), &snapshot);
        assert_ne!(cue_changed.files[0].fingerprint, nfo_changed.files[0].fingerprint);
        std::fs::write(book.join("cover.jpg"), b"image").unwrap();
        let cover_changed = discover(root.path(), &snapshot);
        assert_ne!(cover_changed.files[0].fingerprint, cue_changed.files[0].fingerprint);
        std::fs::write(book.join("Disc 1/03 Epilogue.mp3"), b"more").unwrap();
        let changed = discover(root.path(), &snapshot);
        assert_ne!(changed.files[0].fingerprint, cover_changed.files[0].fingerprint);
        assert_eq!(changed.files.len(), 1);
    }

    #[test]
    fn loose_mp3_in_parent_does_not_hide_nested_books() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("Author");
        std::fs::create_dir_all(parent.join("Series/Book")).unwrap();
        std::fs::write(parent.join("loose.mp3"), b"audio").unwrap();
        std::fs::write(parent.join("Series/Book/01.mp3"), b"chapter").unwrap();
        let snapshot = ScanSnapshot { placements: vec![], work_revision: 0, scan_id: 1, excluded_paths: vec![], directories: vec![], tombstoned_files: vec![], books: vec![] };
        let found = discover(root.path(), &snapshot);
        assert!(found.readable);
        assert_eq!(found.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["Author/Series/Book"]);
        assert!(found.directories.iter().any(|directory| directory.path == "Author"));
    }

    #[test]
    fn excluded_nested_book_does_not_prevent_mp3_folder_grouping() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Story");
        std::fs::create_dir_all(folder.join("Excluded/Other Book")).unwrap();
        std::fs::write(folder.join("01.mp3"), b"chapter").unwrap();
        std::fs::write(folder.join("Excluded/Other Book/01.mp3"), b"other chapter").unwrap();
        let snapshot = ScanSnapshot { placements: vec![], work_revision: 0, scan_id: 1, excluded_paths: vec!["Story/Excluded".into()], directories: vec![], tombstoned_files: vec![], books: vec![] };
        let found = discover(root.path(), &snapshot);
        assert!(found.readable);
        assert_eq!(found.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["Story"]);
    }

    #[test]
    fn excluded_track_in_mixed_directory_keeps_nested_book_visible() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Story");
        std::fs::create_dir_all(folder.join("Other")).unwrap();
        std::fs::write(folder.join("01.mp3"), b"chapter").unwrap();
        std::fs::write(folder.join("Other/book.epub"), b"book").unwrap();
        let snapshot = ScanSnapshot { placements: vec![], work_revision: 0, scan_id: 1, excluded_paths: vec!["Story/02.mp3".into()], directories: vec![], tombstoned_files: vec![], books: vec![] };
        let found = discover(root.path(), &snapshot);
        assert!(found.readable);
        assert_eq!(found.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["Story/Other/book.epub"]);
    }

    #[cfg(unix)]
    #[test]
    fn unrelated_non_utf8_child_does_not_hide_mp3_folder() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Story");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("01.mp3"), b"chapter").unwrap();
        std::fs::create_dir(folder.join(OsString::from_vec(vec![b'x', 0xff]))).unwrap();
        std::fs::write(folder.join(OsString::from_vec(vec![b'y', 0xff])), b"unrelated").unwrap();
        let snapshot = ScanSnapshot { placements: vec![], work_revision: 0, scan_id: 1, excluded_paths: vec![], directories: vec![], tombstoned_files: vec![], books: vec![] };
        let found = discover(root.path(), &snapshot);
        assert!(found.readable);
        assert_eq!(found.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["Story"]);
    }

    #[test]
    fn m4b_next_to_mp3_is_not_hidden_by_folder_grouping() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("Mixed");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("01.mp3"), b"chapter").unwrap();
        std::fs::write(folder.join("Complete.m4b"), b"book").unwrap();
        let snapshot = ScanSnapshot { placements: vec![], work_revision: 0, scan_id: 1, excluded_paths: vec![], directories: vec![], tombstoned_files: vec![], books: vec![] };
        let found = discover(root.path(), &snapshot);
        assert!(found.readable);
        assert_eq!(found.files.iter().map(|file| file.path.as_str()).collect::<Vec<_>>(), ["Mixed/Complete.m4b"]);
    }

    #[test]
    fn m4b_sibling_cover_changes_its_scan_fingerprint() {
        let root = tempfile::tempdir().unwrap();
        let book = root.path().join("Story.m4b");
        std::fs::write(&book, b"audio").unwrap();
        let before = book_fingerprint(&book).unwrap();
        std::fs::write(root.path().join("Story.jpg"), b"cover").unwrap();
        assert_ne!(book_fingerprint(&book).unwrap(), before);
        std::fs::remove_file(root.path().join("Story.jpg")).unwrap();
        assert_eq!(book_fingerprint(&book).unwrap(), before);
    }

    #[test]
    fn deleted_directory_is_not_scanned_as_an_import_destination() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("Misc");
        std::fs::create_dir(&directory).unwrap();
        let id = DirId::new_v4();
        std::fs::write(directory.join(".biblos_uuid"), id.to_string()).unwrap();
        std::fs::write(directory.join("book.epub"), b"book").unwrap();
        let snapshot = ScanSnapshot {
            placements: vec![],
            work_revision: 0,
            scan_id: 1,
            excluded_paths: vec![],
            directories: vec![library_database::ScanKnownDirectory { id, parent_id: ROOT_DIR_ID, name: "Misc".into(), path: Some("Misc".into()), deleted: true }],
            tombstoned_files: vec![],
            books: vec![],
        };
        let scan = discover(root.path(), &snapshot);
        assert!(scan.readable);
        assert!(scan.directories.is_empty());
        assert!(scan.files.is_empty());
    }

    #[test]
    fn deleted_directory_with_audio_tracks_is_not_imported_as_a_book() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("Old audiobook");
        std::fs::create_dir(&directory).unwrap();
        let id = DirId::new_v4();
        std::fs::write(directory.join(".biblos_uuid"), id.to_string()).unwrap();
        std::fs::write(directory.join("01 Opening.mp3"), b"audio").unwrap();
        let snapshot = ScanSnapshot {
            placements: vec![],
            work_revision: 0,
            scan_id: 1,
            excluded_paths: vec![],
            directories: vec![library_database::ScanKnownDirectory { id, parent_id: ROOT_DIR_ID, name: "Old audiobook".into(), path: Some("Old audiobook".into()), deleted: true }],
            tombstoned_files: vec![],
            books: vec![],
        };
        let scan = discover(root.path(), &snapshot);
        assert!(scan.readable);
        assert!(scan.directories.is_empty());
        assert!(scan.files.is_empty());
    }

    #[tokio::test]
    async fn scan_stable_invalid_metadata_is_not_retryable() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("book.epub");
        std::fs::write(&path, b"not an epub").unwrap();
        let error = crate::ingestion::ingest_file(path, "book.epub".into(), &*host()).await.err().unwrap();
        assert!(!retryable(&error));

        {
            use std::io::{Error, ErrorKind::*};
            for kind in [Interrupted, WouldBlock, TimedOut] {
                assert!(retryable(&Error::from(kind)));
            }
            for kind in [InvalidData, PermissionDenied, NotFound, Other] {
                assert!(!retryable(&Error::from(kind)));
            }
        }
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

            books: vec![library_database::ScanKnownBook {
                book_id: BookId::new(&"a".repeat(64)),
                directory: ROOT_DIR_ID,
                name: "book.epub".into(),
                fingerprint: 42,
                inspection_version: Some(book_inspection::inspection_version(BookFormat::Epub)),
                cached_audiobook_format: None,
                cached_pdf: None,
            }],
        };
        let unchanged = inspect(root.path(), &scan, &snapshot, &host()).await;
        assert!(unchanged.readable);
        assert!(unchanged.books.is_empty());
        assert_eq!(scan.files.len(), 1, "unchanged files remain available for mark-seen");

        snapshot.books[0].inspection_version = Some(READER_EPUB_METADATA_VERSION);
        assert!(inspect(root.path(), &scan, &snapshot, &host()).await.readable, "reader EPUB metadata admits this unchanged placement");
        snapshot.books[0].fingerprint = 41;
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable);
        snapshot.books[0].fingerprint = 42;
        snapshot.books[0].inspection_version = None;
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable);
        snapshot.books[0].inspection_version = Some(book_inspection::inspection_version(BookFormat::Epub) - 1);
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable);
        snapshot.books[0].inspection_version = Some(book_inspection::inspection_version(BookFormat::Epub));
        snapshot.books[0].inspection_version = Some(READER_PDF_METADATA_VERSION);
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable, "a PDF marker cannot admit an EPUB");
        snapshot.books[0].inspection_version = Some(READER_EPUB_METADATA_VERSION);
        snapshot.books[0].name = "old-name.epub".into();
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable, "new placements require inspection");
    }

    #[tokio::test]
    async fn reader_pdf_metadata_skips_only_unchanged_pdf_placements() {
        let root = tempfile::tempdir().unwrap();
        let mut scan = FilesystemScan { files: vec![ScanFileObservation { directory: ROOT_DIR_ID, name: "book.pdf".into(), path: "book.pdf".into(), fingerprint: 42 }], readable: true, ..Default::default() };
        let mut snapshot = ScanSnapshot {
            placements: vec![],
            work_revision: 0,
            scan_id: 1,
            excluded_paths: vec![],
            directories: vec![],
            tombstoned_files: vec![],
            books: vec![library_database::ScanKnownBook {
                book_id: BookId::new(&"a".repeat(64)),
                directory: ROOT_DIR_ID,
                name: "book.pdf".into(),
                fingerprint: 42,
                inspection_version: Some(READER_PDF_METADATA_VERSION),
                cached_audiobook_format: None,
                cached_pdf: None,
            }],
        };
        // No file exists: any attempted inspection makes the scan unreadable.
        let unchanged = inspect(root.path(), &scan, &snapshot, &host()).await;
        assert!(unchanged.readable);
        assert!(unchanged.books.is_empty());
        snapshot.books[0].fingerprint = 41;
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable);
        snapshot.books[0].fingerprint = 42;
        snapshot.books[0].inspection_version = None;
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable);
        snapshot.books[0].inspection_version = Some(READER_PDF_METADATA_VERSION);
        snapshot.books[0].name = "book.epub".into();
        scan.files[0].name = "book.epub".into();
        scan.files[0].path = "book.epub".into();
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable, "the PDF admission marker cannot bypass EPUB inspection");
    }

    #[tokio::test]
    async fn synced_audiobook_metadata_skips_verified_unchanged_audio_only() {
        let root = tempfile::tempdir().unwrap();
        // Empty on purpose: opening these files for inspection must fail. The
        // verified fingerprint and complete synced metadata avoid that work.
        std::fs::create_dir(root.path().join("Audio")).unwrap();
        let scan = FilesystemScan { files: vec![ScanFileObservation { directory: ROOT_DIR_ID, name: "Audio".into(), path: "Audio".into(), fingerprint: 42 }], readable: true, ..Default::default() };
        let mut snapshot = ScanSnapshot {
            work_revision: 0,
            scan_id: 1,
            excluded_paths: vec![],
            directories: vec![],
            placements: vec![],
            tombstoned_files: vec![],
            books: vec![library_database::ScanKnownBook {
                book_id: BookId::new(&"a".repeat(64)),
                directory: ROOT_DIR_ID,
                name: "Audio".into(),
                fingerprint: 42,
                inspection_version: None,
                cached_audiobook_format: Some(BookFormat::Mp3Folder),
                cached_pdf: None,
            }],
        };
        let reused = inspect(root.path(), &scan, &snapshot, &host()).await;
        assert!(reused.readable);
        assert!(reused.books.is_empty(), "complete synced facts require no parser call");

        snapshot.books[0].fingerprint = 41;
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable, "changed audio must be inspected");
        snapshot.books[0].fingerprint = 42;
        snapshot.books[0].cached_audiobook_format = None;
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable, "incomplete metadata must be inspected");
        snapshot.books[0].cached_audiobook_format = Some(BookFormat::M4b);
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable, "metadata must describe the observed format");
        snapshot.books[0].cached_audiobook_format = Some(BookFormat::Mp3Folder);
        snapshot.books[0].inspection_version = Some(book_inspection::inspection_version(BookFormat::Mp3Folder) - 1);
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable, "explicitly stale inspections still need upgrading");
    }
}
