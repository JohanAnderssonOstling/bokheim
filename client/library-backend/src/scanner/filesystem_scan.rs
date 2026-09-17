//! Filesystem-only catalogue discovery.
//!
//! This module deliberately has no database, event sink, worker thread, or
//! retry policy. The session supplies a database snapshot and decides what to
//! do with the returned observations.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use library_database::{FilesystemScan, InspectedFilesystemScan, ScanCatalogSnapshot, ScanDirectoryObservation, ScanFileObservation, ScannedBook};
use book_model::BookFormat;
use sync_common::{ContentHash, DirId, ROOT_DIR_ID};

fn excluded(snapshot: &ScanCatalogSnapshot, path: &Path) -> bool {
    snapshot.excluded_paths.iter().any(|excluded| {
        let excluded = Path::new(excluded);
        path == excluded || path.starts_with(excluded)
    })
}

fn supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| BookFormat::from_extension(&extension.to_ascii_lowercase()).is_some())
}

/// Walk `root` and return directory/file observations. Marker creation is
/// physical filesystem work; all catalog decisions are deferred to the caller.
pub fn discover(root: impl Into<PathBuf>, snapshot: &ScanCatalogSnapshot) -> FilesystemScan {
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
            if entry.is_dir() { walk.skip_current_dir(); }
            continue;
        }
        let relative = match entry.path().strip_prefix(&root) {
            Ok(path) => path,
            Err(_) => {
                result.readable = false;
                if entry.is_dir() { walk.skip_current_dir(); }
                continue;
            }
        };
        if excluded(snapshot, relative) {
            if entry.is_dir() { walk.skip_current_dir(); }
            continue;
        }
        while parents.len() > entry.depth() { parents.pop(); }
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

/// Inspects the discovered files without consulting the catalog. Failed files
/// remain visible through `readable = false`; the session decides whether that
/// permits a placement cleanup pass.
pub fn inspect(root: impl Into<PathBuf>, scan: &FilesystemScan) -> InspectedFilesystemScan {
    use std::io::Read as _;

    let root = root.into();
    let mut result = InspectedFilesystemScan { readable: scan.readable, ..InspectedFilesystemScan::default() };
    for file in &scan.files {
        let path = root.join(&file.path);
        let inspected = (|| -> Result<ScannedBook, std::io::Error> {
            let content_hash = book_identity::ensure(&path)?;
            let fingerprint = library_files::filesystem::fingerprint(&path)?;
            let mut reader = library_files::filesystem::open(&path)?;
            let mut hasher = blake3::Hasher::new();
            let mut bytes = [0_u8; 64 * 1024];
            let mut size_bytes = 0_u64;
            loop {
                let read = reader.read(&mut bytes)?;
                if read == 0 { break; }
                hasher.update(&bytes[..read]);
                size_bytes = size_bytes.saturating_add(read as u64);
            }
            if size_bytes == 0 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "empty book")); }
            let extension = path.extension().and_then(|extension| extension.to_str()).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "unsupported book"))?.to_ascii_lowercase();
            Ok(ScannedBook { directory: file.directory, name: file.name.clone(), path: file.path.clone(), fingerprint, content_hash, checksum: ContentHash::new(hasher.finalize().to_hex().as_str()), size_bytes, extension })
        })();
        match inspected {
            Ok(book) => result.books.push(book),
            Err(error) => {
                log::warn!("scanner could not inspect {}: {error}", path.display());
                result.readable = false;
            }
        }
    }
    result
}
