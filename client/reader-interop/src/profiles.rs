//! Default desktop profile locations. Discovery never creates reader files.
use serde_json::Value;
use std::{
    io,
    path::{Path, PathBuf},
};

pub const FOLIATE_APP_ID: &str = "com.github.johnfactotum.Foliate";
pub const READEST_APP_ID: &str = "com.bilingify.readest";

/// Native, Flatpak and Snap data/config pairs for the current platform.
fn directories(app: &str, snap: &str) -> io::Result<Vec<(PathBuf, PathBuf)>> {
    let data = dirs::data_dir().ok_or_else(|| io::Error::other("could not locate reader application data"))?;
    let config = dirs::config_dir().ok_or_else(|| io::Error::other("could not locate reader settings"))?;
    #[allow(unused_mut)]
    let mut roots = vec![(data.join(app), config.join(app))];
    #[cfg(target_os = "linux")]
    if let Some(home) = dirs::home_dir() {
        roots.push((home.join(".var/app").join(app).join("data").join(app), home.join(".var/app").join(app).join("config").join(app)));
        roots.push((home.join(format!("snap/{snap}/current/.local/share")).join(app), home.join(format!("snap/{snap}/current/.config")).join(app)));
    }
    #[cfg(not(target_os = "linux"))]
    let _ = snap;
    Ok(roots)
}
pub fn foliate_directories() -> io::Result<Vec<PathBuf>> {
    Ok(directories(FOLIATE_APP_ID, "foliate")?.into_iter().map(|(data, _)| data).collect())
}
pub fn readest_directories() -> io::Result<Vec<(PathBuf, PathBuf)>> {
    directories(READEST_APP_ID, "readest")
}

/// The caller retains its read policy (including export destination limits).
pub fn readest_books_directory(data: &Path, settings: Option<&[u8]>) -> io::Result<PathBuf> {
    if let Some(bytes) = settings {
        let settings: Value = serde_json::from_slice(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if let Some(root) = settings.get("localBooksDir") {
            let path =
                root.as_str().filter(|root| !root.is_empty()).map(PathBuf::from).filter(|path| path.is_absolute()).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Readest book directory must be a nonempty absolute path"))?;
            return Ok(path);
        }
    }
    Ok(data.join("Readest/Books"))
}
