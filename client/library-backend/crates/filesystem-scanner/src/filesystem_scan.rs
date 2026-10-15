//! Filesystem-only inventory discovery.
//!
//! The session supplies an immutable database snapshot. Inspection retries
//! unstable files locally; the session owns scan scheduling and commits.

use crate::ingestion::fingerprint as book_fingerprint;
use crate::ingestion::{ingest_audiobook_folder, ingest_file};
use futures_util::StreamExt;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use book_model::BookFormat;
use library_database::{FilesystemScan, InspectedFilesystemScan, ScanDirectoryObservation, ScanFileObservation, ScanSnapshot, ScannedBook};
use sync_common::{DirId, ROOT_DIR_ID};

pub const PUBLISH_BATCH_SIZE: usize = 32;
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

fn other_book_in_audiobook_candidate(folder: &Path, root: &Path, excluded: impl Fn(&Path) -> bool) -> std::io::Result<bool> {
    let mut children = Vec::new();
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if entry.path().strip_prefix(root).is_ok_and(&excluded) {
            continue;
        }
        if entry.file_type()?.is_file() && supported(&entry.path()) {
            return Ok(true);
        }
        if entry.file_type()?.is_dir() {
            let disc = entry.file_name().to_str().is_some_and(audiobook_folder::is_disc_folder_name);
            children.push((entry.path(), disc));
        }
    }
    while let Some((directory, direct_disc)) = children.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if entry.path().strip_prefix(root).is_ok_and(&excluded) {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                children.push((entry.path(), false));
            } else if kind.is_file() {
                let path = entry.path();
                let mp3 = path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("mp3"));
                if supported(&path) || (mp3 && !direct_disc) {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
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
        let lower = name.to_ascii_lowercase();
        Path::new(&*name).extension().and_then(|extension| extension.to_str()).is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "mp3" | "nfo" | "cue"))
            || ["cover.jpg", "cover.jpeg", "cover.png", "cover.webp", "folder.jpg", "folder.jpeg", "folder.png", "folder.webp"].contains(&lower.as_str())
    })
}

fn folder_fingerprint(folder: &Path, tracks: &[audiobook_archive::Track]) -> std::io::Result<u64> {
    library_files::filesystem::audiobook_fingerprint(folder, tracks)
}

/// Whether a dropped directory is one audiobook rather than a collection
/// containing other books. Use the same boundaries as filesystem discovery.
pub fn is_audiobook_folder(folder: &Path) -> std::io::Result<bool> {
    let tracks = audiobook_folder::discover(folder);
    if tracks.as_ref().is_ok_and(|tracks| tracks.is_empty()) || other_book_in_audiobook_candidate(folder, folder, |_| false)? {
        return Ok(false);
    }
    Ok(!tracks?.is_empty())
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
            if library_files::filesystem::read_to_string(&marker).ok().and_then(|marker| DirId::parse_str(marker.trim()).ok()).is_some_and(|id| known.get(&id).is_some_and(|known| known.deleted)) {
                walk.skip_current_dir();
                continue;
            }
            match audiobook_folder::discover(entry.path()) {
                Ok(tracks) if !tracks.is_empty() && excluded_audiobook_member(relative, &snapshot.excluded_paths) && !other_book_in_audiobook_candidate(entry.path(), &root, |path| excluded(snapshot, path)).unwrap_or(true) => {
                    walk.skip_current_dir();
                    continue;
                }
                Ok(tracks) if !tracks.is_empty() && !other_book_in_audiobook_candidate(entry.path(), &root, |path| excluded(snapshot, path)).unwrap_or(true) => {
                    match (relative.to_str(), folder_fingerprint(entry.path(), &tracks)) {
                        (Some(path), Ok(fingerprint)) => result.files.push(ScanFileObservation { directory: parent_id, name, path: path.to_owned(), fingerprint }),
                        _ => result.readable = false,
                    }
                    walk.skip_current_dir();
                    continue;
                }
                Ok(_) => {}
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
            match book_fingerprint(entry.path()) {
                Ok(fingerprint) => result.files.push(ScanFileObservation { directory: parent_id, name: name.to_owned(), path: path.to_owned(), fingerprint }),
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
    let root = root.into();
    let mut result = InspectedFilesystemScan { readable: scan.readable, ..InspectedFilesystemScan::default() };
    let known = snapshot.books.iter().map(|book| ((book.directory, book.name.as_str()), book)).collect::<HashMap<_, _>>();
    let candidates = scan.files.iter().filter(|file| {
        // Keep all observations for mark-seen, but only read and parse books
        // whose placement is new or whose bytes may have changed. Verified
        // audiobooks can reuse complete synced playback facts; other legacy
        // records without an inspection must still be populated once.
        let format = if root.join(&file.path).is_dir() {
            Some(BookFormat::Mp3Folder)
        } else {
            Path::new(&file.name).extension().and_then(|extension| extension.to_str()).and_then(|extension| BookFormat::from_extension(&extension.to_ascii_lowercase()))
        };
        let version = format.map(book_inspection::inspection_version);
        !known.get(&(file.directory, file.name.as_str())).is_some_and(|book| {
            book.fingerprint == file.fingerprint && version.is_some() && (book.inspection_version == version || (book.inspection_version.is_none() && book.cached_audiobook_format.is_some() && book.cached_audiobook_format == format))
        })
    });
    let mut inspections = futures_util::stream::iter(candidates.map(|file| {
        let path = root.join(&file.path);
        let root = root.clone();
        let host = host.clone();
        async move {
            for attempt in 0..3 {
                match (async |host: &dyn cpu_host::CpuHost, path: PathBuf, root: &Path, excluded_paths: &[String], file: &ScanFileObservation| -> std::io::Result<ScannedBook> {
                    let ingested = if path.is_dir() {
                        if excluded_audiobook_member(path.strip_prefix(root).unwrap_or(&path), excluded_paths) {
                            return Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "audiobook contains a temporarily excluded file"));
                        }
                        ingest_audiobook_folder(path, file.name.clone(), host).await?
                    } else {
                        ingest_file(path, file.name.clone(), host).await?
                    };
                    let crate::ingestion::IngestedFile { content_hash, size_bytes, fingerprint, format, inspection } = ingested;
                    let extension = format.canonical_extension().to_owned();
                    Ok(ScannedBook {
                        directory: file.directory,
                        name: file.name.clone(),
                        path: file.path.clone(),
                        fingerprint,
                        content_hash,
                        size_bytes,
                        extension,
                        inspection,
                        inspection_version: book_inspection::inspection_version(format),
                    })
                })(&*host, path.clone(), &root, &snapshot.excluded_paths, file)
                .await
                {
                    Ok(book) => return Ok(book),
                    Err(error) if attempt == 2 || !retryable(&error) => {
                        log::warn!("scanner skipped {} after {} inspection attempts: {error}", path.display(), attempt + 1);
                        return Err((file.path.clone(), error.to_string()));
                    }
                    Err(_) => client_platform_runtime::executor::sleep(std::time::Duration::from_millis(100)).await,
                }
            }
            unreachable!("inspection attempts return a result")
        }
    }))
    .buffered(MAX_CONCURRENT_INSPECTIONS);
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
    use sync_common::ContentHash;

    /// Inspection dispatch is unreachable here: fixtures exercise selection and
    /// failure plumbing with the book file absent, so hashing fails first.
    struct FailingHost;
    impl cpu_host::CpuHost for FailingHost {
        fn book(&self, _task: cpu_host::BookTask, _reader: cpu_host::BoxedCpuReader) -> client_platform_runtime::executor::BoxedBackendFuture<'_, Result<cpu_host::CpuResponse, client_runtime::BackendError>> {
            Box::pin(async { Err("no CPU host in scanner tests".into()) })
        }
        fn request(&self, _request: cpu_host::CpuRequest) -> client_platform_runtime::executor::BoxedBackendFuture<'_, Result<cpu_host::CpuResponse, client_runtime::BackendError>> {
            Box::pin(async { Err("no CPU host in scanner tests".into()) })
        }
        fn generate_thumbnail(&self, _extension: String, _reader: cpu_host::BoxedCpuReader) -> client_platform_runtime::executor::BoxedBackendFuture<'_, Result<Option<cpu_host::ThumbnailBytes>, client_runtime::BackendError>> {
            Box::pin(async { Err("no CPU host in scanner tests".into()) })
        }
    }
    fn host() -> Arc<dyn cpu_host::CpuHost> {
        Arc::new(FailingHost)
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
        let error = ingest_file(path, "book.epub".into(), &*host()).await.err().unwrap();
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
                content_hash: ContentHash::new(&"a".repeat(64)),
                directory: ROOT_DIR_ID,
                name: "book.epub".into(),
                fingerprint: 42,
                inspection_version: Some(book_inspection::inspection_version(BookFormat::Epub)),
                cached_audiobook_format: None,
            }],
        };
        let unchanged = inspect(root.path(), &scan, &snapshot, &host()).await;
        assert!(unchanged.readable);
        assert!(unchanged.books.is_empty());
        assert_eq!(scan.files.len(), 1, "unchanged files remain available for mark-seen");

        snapshot.books[0].fingerprint = 41;
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable);
        snapshot.books[0].fingerprint = 42;
        snapshot.books[0].inspection_version = None;
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable);
        snapshot.books[0].inspection_version = Some(book_inspection::inspection_version(BookFormat::Epub) - 1);
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable);
        snapshot.books[0].inspection_version = Some(book_inspection::inspection_version(BookFormat::Epub));
        snapshot.books[0].name = "old-name.epub".into();
        assert!(!inspect(root.path(), &scan, &snapshot, &host()).await.readable, "new placements require inspection");
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
                content_hash: ContentHash::new(&"a".repeat(64)),
                directory: ROOT_DIR_ID,
                name: "Audio".into(),
                fingerprint: 42,
                inspection_version: None,
                cached_audiobook_format: Some(BookFormat::Mp3Folder),
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
