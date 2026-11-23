//! The destination for future native libraries, independent of the device registry.
#[cfg(not(target_arch = "wasm32"))]
use client_platform_runtime::file_storage as files;
#[cfg(not(target_arch = "wasm32"))]
use client_platform_runtime::filesystem as fs;
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
    fs::create_dir_all(path)?;
    let path = fs::canonicalize(&path)?;
    if !fs::is_dir(&path) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "library root is not a directory"));
    }
    let path = path.to_string_lossy().into_owned();
    #[cfg(target_os = "android")]
    let previous = library_save_location(root)?;
    remember_library_save_location(root, &path)?;
    #[cfg(target_os = "android")]
    if fs::is_document_path(Path::new(&path)) {
        if let Err(error) = fs::set_default_document_root(&path) {
            // Keep the launcher and the backend on the same saved destination.
            let restored = match previous {
                Some(previous) => remember_library_save_location(root, &previous),
                None => fs::remove_file(root.join(FILENAME)),
            };
            return Err(match restored {
                Ok(()) => error,
                Err(rollback) => io::Error::other(format!("{error}; could not restore the previous library location: {rollback}")),
            });
        }
    }
    Ok(path)
}

/// Restore the platform's already-authorized location without recreating a
/// missing volume. Availability is handled by the library's normal root checks.
#[cfg(not(target_arch = "wasm32"))]
pub fn remember_library_save_location(root: &Path, path: &str) -> io::Result<()> {
    if !Path::new(path).is_absolute() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "library save location must be absolute"));
    }
    files::write_atomic_sync(&root.join(FILENAME), &serde_json::to_vec(path).map_err(io::Error::other)?)
}

impl crate::Client {
    pub async fn default_library_save_location(&self, fallback: &str) -> io::Result<Option<String>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            use client_platform_runtime::executor::run_blocking;
            let root = self.root.clone();
            let saved = run_blocking(move || library_save_location(&root)).await.map_err(io::Error::other)??;
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
            use client_platform_runtime::storage_queue;
            let root = self.root.clone();
            storage_queue::run(move || set_library_save_location(&root, &path)).await.map_err(io::Error::other)?
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            Err(io::Error::other("browser storage has no filesystem save location"))
        }
    }
}
