//! Concrete native filesystem access for a library root.
//!
//! This module deliberately exposes bytes, paths, and directory entries only.
//! It does not classify books or persist scan decisions.

use client_platform_native::filesystem::retry_transient;
#[cfg(not(target_arch = "wasm32"))]
use client_platform_runtime::filesystem::temporary;
#[cfg(not(target_arch = "wasm32"))]
use client_platform_runtime::filesystem::walk;
use client_platform_runtime::filesystem::{self as fs, File};
use std::ffi::OsStr;
use std::io::{self, Error, ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub struct DirectoryWalk {
    entries: walk::IntoIter,
}

pub struct DirectoryEntry {
    path: PathBuf,
    file_name: std::ffi::OsString,
    directory: bool,
    file: bool,
    symlink: bool,
    depth: usize,
    length: Option<u64>,
}

impl DirectoryEntry {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn file_name(&self) -> &OsStr {
        &self.file_name
    }
    pub fn is_dir(&self) -> bool {
        self.directory
    }
    pub fn is_file(&self) -> bool {
        self.file
    }
    pub fn is_symlink(&self) -> bool {
        self.symlink
    }
    pub fn depth(&self) -> usize {
        self.depth
    }
    /// `None` means the entry could not be stat'ed without aborting the walk.
    pub fn length(&self) -> Option<u64> {
        self.length
    }
}

impl DirectoryWalk {
    pub fn next(&mut self) -> Option<Result<DirectoryEntry, Error>> {
        self.entries.next().map(|entry| {
            let entry = entry.map_err(|error| {
                let message = error.to_string();
                error.into_io_error().unwrap_or_else(|| Error::other(message))
            })?;
            let kind = entry.file_type();
            Ok(DirectoryEntry {
                path: entry.path().to_path_buf(),
                file_name: entry.file_name().to_os_string(),
                directory: kind.is_dir(),
                file: kind.is_file(),
                symlink: kind.is_symlink(),
                depth: entry.depth(),
                length: entry.metadata().ok().map(|metadata| metadata.len()),
            })
        })
    }

    pub fn skip_current_dir(&mut self) {
        self.entries.skip_current_dir();
    }
}

pub fn walk(root: impl AsRef<Path>) -> DirectoryWalk {
    DirectoryWalk { entries: walk::WalkDir::new(root).follow_links(false).min_depth(1).into_iter() }
}

pub fn read_to_string(path: impl AsRef<Path>) -> std::io::Result<String> {
    retry_transient(|| fs::read_to_string(path.as_ref()))
}
pub fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> std::io::Result<()> {
    fs::write(path, bytes)
}
pub fn open(path: impl AsRef<Path>) -> std::io::Result<File> {
    retry_transient(|| fs::open(path.as_ref()))
}

pub fn fingerprint(path: impl AsRef<Path>) -> std::io::Result<u64> {
    client_platform_native::file_fingerprint::file_fingerprint(path)
}

/// Fingerprint the ordered tracks as one scanned placement. Both the scanner
/// and download publisher use this so a fresh download is already up to date.
pub fn audiobook_fingerprint(folder: &Path, tracks: &[audiobook_archive::Track]) -> std::io::Result<u64> {
    let mut hasher = blake3::Hasher::new();
    for track in tracks {
        hasher.update(track.name.as_bytes());
        hasher.update(&fingerprint(&track.path)?.to_le_bytes());
    }
    let mapping = folder.join(audiobook_folder::PATH_MAPPING_FILE);
    if fs::try_exists(&mapping)? {
        hasher.update(audiobook_folder::PATH_MAPPING_FILE.as_bytes());
        hasher.update(&fingerprint(mapping)?.to_le_bytes());
    }
    if let Some(nfo) = audiobook_folder::discover_nfo(folder)? {
        hasher.update(nfo.name.as_bytes());
        hasher.update(&fingerprint(&nfo.path)?.to_le_bytes());
    }
    if let Some(cue) = audiobook_folder::discover_cue(folder)? {
        hasher.update(cue.name.as_bytes());
        hasher.update(&fingerprint(&cue.path)?.to_le_bytes());
    }
    if let Some(cover) = audiobook_folder::discover_cover(folder)? {
        hasher.update(cover.name.as_bytes());
        hasher.update(&fingerprint(&cover.path)?.to_le_bytes());
    }
    let [a, b, c, d, e, f, g, h, ..] = *hasher.finalize().as_bytes();
    Ok(u64::from_le_bytes([a, b, c, d, e, f, g, h]))
}

/// Places a file or complete folder before any book inspection begins.
/// A path already in this directory is a retry and stays in place.
pub fn place_into_directory(source: &Path, directory: &Path, occupied: &[String], moving: bool) -> io::Result<PathBuf> {
    place_into_directory_with_intent(source, directory, occupied, moving, |_| Ok(()))
}

/// Report total bytes for the source file or folder, including nested files.
/// Renames have no transfer work to report.
pub fn place_into_directory_with_progress(source: &Path, directory: &Path, occupied: &[String], moving: bool, progress: &mut dyn FnMut(&Path, u64, u64)) -> io::Result<PathBuf> {
    place_entry(source, directory, occupied, moving, true, &mut |_| Ok(()), progress)
}

/// Records each candidate destination before publishing it, so an owner can
/// recover a move interrupted between the rename and its database acknowledgement.
pub fn place_into_directory_with_intent(source: &Path, directory: &Path, occupied: &[String], moving: bool, mut before_publish: impl FnMut(&Path) -> io::Result<()>) -> io::Result<PathBuf> {
    place_entry(source, directory, occupied, moving, true, &mut before_publish, &mut |_, _, _| {})
}

/// A same-filesystem move only. Never silently turn a failed rename into
/// copying and deleting the caller's source.
pub fn rename_into_directory_with_intent(source: &Path, directory: &Path, occupied: &[String], mut before_publish: impl FnMut(&Path) -> io::Result<()>) -> io::Result<PathBuf> {
    place_entry(source, directory, occupied, true, false, &mut before_publish, &mut |_, _, _| {})
}

fn place_entry(source: &Path, directory: &Path, occupied: &[String], moving: bool, copy_fallback: bool, before_publish: &mut dyn FnMut(&Path) -> io::Result<()>, progress: &mut dyn FnMut(&Path, u64, u64)) -> io::Result<PathBuf> {
    let metadata = fs::symlink_metadata(source)?;
    if !metadata.is_file() && !metadata.is_dir() {
        return Err(Error::other("only regular files and folders can be added to a library"));
    }
    let source = fs::canonicalize(&source)?;
    let directory = fs::canonicalize(&directory)?;
    if source.parent() == Some(directory.as_path()) {
        return Ok(source);
    }
    if directory.starts_with(&source) {
        return Err(Error::other("a folder cannot be placed inside itself"));
    }
    let name = source.file_name().and_then(OsStr::to_str).ok_or_else(|| Error::other("invalid file name"))?;
    let mut names = occupied.to_vec();
    names.extend(fs::read_dir(&directory)?.map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned())).collect::<Result<Vec<_>, _>>()?);
    let candidates: Box<dyn Iterator<Item = String> + '_> =
        if metadata.is_dir() { Box::new(library_replica::folder_name_candidates(name, names.iter().map(String::as_str))) } else { Box::new(library_replica::file_name_candidates(name, names.iter().map(String::as_str))) };
    let mut staging: Option<temporary::TempDir> = None;
    for name in candidates {
        let destination = directory.join(name);
        if fs::symlink_metadata(&destination).is_ok() {
            continue;
        }
        before_publish(&destination)?;
        if moving && staging.is_none() {
            match rename_entry(&source, &destination) {
                Ok(()) => return Ok(destination),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) if copy_fallback && error.kind() == ErrorKind::CrossesDevices => {}
                Err(error) => return Err(error),
            }
        }
        // A complete file or folder is copied under a hidden directory on the
        // destination filesystem. The scanner skips it, including after a crash.
        let temporary = match &mut staging {
            Some(temporary) => temporary,
            vacant @ None => {
                let temporary = temporary::Builder::new().prefix(".bokheim-copy-").tempdir_in(&directory)?;
                let mut total = if metadata.is_file() { metadata.len() } else { 0 };
                if metadata.is_dir() {
                    let mut entries = walk(&source);
                    while let Some(entry) = entries.next() {
                        let entry = entry?;
                        if entry.is_file() {
                            total += entry.length().ok_or_else(|| Error::other("could not determine source file size"))?;
                        }
                    }
                }
                let mut copied = 0;
                progress(&source, copied, total);
                copy_entry_with_progress(&source, &temporary.path().join("contents"), &mut |bytes| {
                    copied += bytes;
                    progress(&source, copied, total);
                })?;
                vacant.insert(temporary)
            }
        };
        let staged = temporary.path().join("contents");
        match rename_entry(&staged, &destination) {
            Ok(()) => {
                // Cross-filesystem moves delete the original only after the
                // completed copy has been renamed into its final location.
                if moving {
                    if metadata.is_dir() {
                        fs::remove_dir_all(&source)?;
                    } else {
                        fs::remove_file(&source)?;
                    }
                }
                return Ok(destination);
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    unreachable!("numeric name suffix space is unbounded")
}

pub(crate) fn rename_entry(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(target_os = "android")]
    {
        // Filename candidates and destination checks above handle conflicts.
        // Shared storage may reject RENAME_NOREPLACE, so use regular rename.
        if fs::is_document_path(source) || fs::is_document_path(destination) { fs::rename_noclobber(source, destination) } else { fs::rename(source, destination) }
    }
    #[cfg(not(target_os = "android"))]
    {
        client_platform_native::filesystem::rename_noclobber(source, destination)
    }
}

pub(crate) fn copy_entry(source: &Path, destination: &Path) -> io::Result<()> {
    copy_entry_with_progress(source, destination, &mut |_| {})
}

fn copy_entry_with_progress(source: &Path, destination: &Path, progress: &mut dyn FnMut(u64)) -> io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.is_dir() {
        fs::create_dir(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_entry_with_progress(&entry.path(), &destination.join(entry.file_name()), progress)?;
        }
    } else if metadata.is_file() {
        let mut reported = 0;
        copy_file_with_progress(source, destination, &mut |copied, _| {
            progress(copied - reported);
            reported = copied;
        })?;
    } else {
        return Err(Error::other("cannot copy symbolic links or special files"));
    }
    Ok(())
}

/// Copy into private staging, flushing the complete file before publication.
/// Throttle observations so a fast disk cannot flood the presentation queue.
pub fn copy_file_with_progress(source: &Path, destination: &Path, progress: &mut dyn FnMut(u64, u64)) -> io::Result<()> {
    let mut reader = fs::open(source)?;
    let metadata = reader.metadata()?;
    let total = metadata.len();
    let mut writer = fs::OpenOptions::new().write(true).create_new(true).open(destination)?;
    let mut copied = 0;
    let mut reported = Instant::now();
    progress(0, total);
    loop {
        let count = io::copy(&mut reader.by_ref().take(1024 * 1024), &mut writer)?;
        if count == 0 {
            break;
        }
        copied += count;
        if reported.elapsed() >= Duration::from_millis(100) {
            progress(copied, total);
            reported = Instant::now();
        }
    }
    if copied != total {
        return Err(Error::other("source size changed while copying"));
    }
    fs::set_permissions(destination, metadata.permissions())?;
    writer.sync_all()?;
    progress(copied, total);
    Ok(())
}
