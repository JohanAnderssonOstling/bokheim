use super::DirectoryEntry;
use crate::executor::run_blocking;
use crate::storage_queue;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Write};
use std::path::Path;
use std::time::UNIX_EPOCH;

pub type Source = File;

pub fn read_sync(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub async fn read(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let path = path.to_owned();
    run_blocking(move || read_sync(&path)).await.map_err(io::Error::other)?
}

/// Publish a complete file by atomically replacing its previous contents.
pub fn write_atomic_sync(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
    staged.write_all(bytes)?;
    staged.as_file().sync_all()?;
    staged.persist(path).map_err(|error| error.error)?;
    Ok(())
}

/// Preserve I/O error kinds for callers that present actionable storage failures.
pub async fn write_atomic(path: &Path, bytes: Vec<u8>) -> io::Result<()> {
    let path = path.to_owned();
    storage_queue::run(move || write_atomic_sync(&path, &bytes)).await.map_err(io::Error::other)?
}

pub async fn open(path: &Path) -> io::Result<Source> {
    let path = path.to_owned();
    run_blocking(move || File::open(path)).await.map_err(io::Error::other)?
}

pub async fn remove_directory(path: &Path) -> io::Result<()> {
    let path = path.to_owned();
    storage_queue::run(move || match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    })
    .await.map_err(io::Error::other)?
}

pub async fn file_size(path: &Path) -> io::Result<Option<u64>> {
    let path = path.to_owned();
    run_blocking(move || match fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.len())),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    })
    .await.map_err(io::Error::other)?
}

/// List regular files directly inside a directory; a missing directory is empty.
pub async fn list_directory(path: &Path) -> io::Result<Vec<DirectoryEntry>> {
    let path = path.to_owned();
    run_blocking(move || -> io::Result<Vec<DirectoryEntry>> {
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            let modified_ms = metadata.modified()?.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(u128::from(u64::MAX)) as u64;
            files.push(DirectoryEntry { name: entry.file_name().to_string_lossy().into_owned(), size: metadata.len(), modified_ms });
        }
        Ok(files)
    })
    .await.map_err(io::Error::other)?
}

pub async fn remove_file(path: &Path) -> io::Result<()> {
    let path = path.to_owned();
    storage_queue::run(move || match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    })
    .await.map_err(io::Error::other)?
}
