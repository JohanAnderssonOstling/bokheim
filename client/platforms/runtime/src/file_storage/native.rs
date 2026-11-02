use crate::storage_queue::{self, Priority};
use std::fs::{self, File};
use std::io::{self, ErrorKind, Write};
use std::path::Path;

pub type Source = File;

pub fn read_sync(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub async fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let path = path.to_owned();
    storage_queue::run(Priority::Interactive, move || read_sync(&path)).await?.map_err(|error| error.to_string())
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

pub async fn write_atomic(path: &Path, bytes: Vec<u8>) -> Result<(), String> {
    let path = path.to_owned();
    storage_queue::run(Priority::Interactive, move || write_atomic_sync(&path, &bytes)).await?.map_err(|error| error.to_string())
}

pub async fn open(path: &Path) -> Result<Source, String> {
    File::open(path).map_err(|error| error.to_string())
}

pub async fn remove_directory(path: &Path) -> Result<(), String> {
    let path = path.to_owned();
    storage_queue::run(Priority::Interactive, move || match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    })
    .await?
}
