//! The destination for future native libraries, independent of the device registry.
#[cfg(not(target_arch = "wasm32"))]
use client_platform_runtime::file_storage as files;
use std::io;
#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;

#[cfg(not(target_arch = "wasm32"))]
const FILENAME: &str = "library-save-location.json";

#[cfg(not(target_arch = "wasm32"))]
pub fn library_save_location(root: &Path) -> io::Result<Option<String>> {
    files::read_sync(&root.join(FILENAME))?.map(|bytes| serde_json::from_slice(&bytes).map_err(io::Error::other)).transpose()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn set_library_save_location(root: &Path, path: &str) -> io::Result<String> {
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "choose an absolute library save location"));
    }
    std::fs::create_dir_all(path)?;
    let path = path.canonicalize()?;
    if !path.is_dir() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "library root is not a directory"));
    }
    let path = path.to_string_lossy().into_owned();
    files::write_atomic_sync(&root.join(FILENAME), &serde_json::to_vec(&path).map_err(io::Error::other)?)?;
    Ok(path)
}

/// Preserve a saved choice without accessing a possibly disconnected destination.
#[cfg(not(target_arch = "wasm32"))]
pub fn migrate_library_save_location(root: &Path, legacy: &[u8]) -> io::Result<()> {
    if library_save_location(root)?.is_none() {
        let path = std::str::from_utf8(legacy).map_err(io::Error::other)?;
        files::write_atomic_sync(&root.join(FILENAME), &serde_json::to_vec(path).map_err(io::Error::other)?)?;
    }
    Ok(())
}

impl crate::Client {
    pub async fn default_library_save_location(&self, fallback: &str) -> io::Result<Option<String>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use client_platform_runtime::storage_queue::{self, Priority};
            let root = self.root.clone();
            let saved = storage_queue::run(Priority::Interactive, move || library_save_location(&root)).await.map_err(io::Error::other)??;
            Ok(Some(saved.unwrap_or_else(|| fallback.to_owned())))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = fallback;
            Ok(None)
        }
    }

    pub async fn set_default_library_save_location(&self, path: String) -> io::Result<String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use client_platform_runtime::storage_queue::{self, Priority};
            let root = self.root.clone();
            storage_queue::run(Priority::Interactive, move || set_library_save_location(&root, &path)).await.map_err(io::Error::other)?
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            Err(io::Error::other("browser storage has no filesystem save location"))
        }
    }
}
