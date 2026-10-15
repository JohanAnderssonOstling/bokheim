//! Native taxonomy resource. SQLite opens this file directly with a bounded page cache.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const RELEASE_ID: u64 = include!(concat!(env!("OUT_DIR"), "/taxonomy-release.rs"));
static PATH: OnceLock<PathBuf> = OnceLock::new();

/// Android supplies the extracted APK resource before opening any library.
/// Register the packaged file's location; SQLite opens it when it is needed.
pub fn set_path(path: &Path) -> Result<(), String> {
    let path = path.to_path_buf();
    if let Some(previous) = PATH.get() {
        return if previous == &path { Ok(()) } else { Err("bundled taxonomy path is already configured".into()) };
    }
    match PATH.set(path) {
        Ok(()) => Ok(()),
        // Several library workers may discover the same file concurrently.
        Err(path) if PATH.get() == Some(&path) => Ok(()),
        Err(_) => Err("bundled taxonomy path is already configured".into()),
    }
}

pub fn path() -> Result<PathBuf, String> {
    if let Some(path) = PATH.get() {
        return Ok(path.clone());
    }
    let executable = std::env::current_exe().map_err(|error| format!("locate bundled taxonomy: {error}"))?;
    let directory = executable.parent().ok_or("executable has no parent directory")?;
    let directory = if matches!(directory.file_name().and_then(|name| name.to_str()), Some("deps" | "examples")) { directory.parent().unwrap_or(directory) } else { directory };
    let path = directory.join("taxonomy.sqlite3");
    if path.is_file() {
        set_path(&path)?;
        return Ok(PATH.get().expect("bundled path configured").clone());
    }
    // Debug builds also support tools whose executable is in a separate target directory.
    #[cfg(debug_assertions)]
    {
        let generated = PathBuf::from(concat!(env!("OUT_DIR"), "/unified-taxonomy-client-v2.sqlite3"));
        if generated.is_file() {
            set_path(&generated)?;
            return Ok(PATH.get().expect("bundled path configured").clone());
        }
    }
    Err(format!("bundled taxonomy resource is missing: {}", path.display()))
}
