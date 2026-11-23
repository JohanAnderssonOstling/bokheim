//! Filesystem access for native paths and Android user-granted document trees.
//! Document paths are opaque locators; callers must use this module rather than
//! passing them to OS path operations. Open files remain ordinary owned handles.
#[cfg(target_os = "android")]
mod documents;
#[cfg(target_os = "android")]
pub use documents::*;
#[cfg(not(target_os = "android"))]
pub use std::fs::*;

#[cfg(not(target_os = "android"))]
pub fn open(path: impl AsRef<std::path::Path>) -> std::io::Result<File> {
    File::open(path)
}
#[cfg(not(target_os = "android"))]
pub fn create(path: impl AsRef<std::path::Path>) -> std::io::Result<File> {
    File::create(path)
}
pub fn is_dir(path: impl AsRef<std::path::Path>) -> bool {
    metadata(path).is_ok_and(|metadata| metadata.is_dir())
}
pub fn is_file(path: impl AsRef<std::path::Path>) -> bool {
    metadata(path).is_ok_and(|metadata| metadata.is_file())
}
pub fn try_exists(path: impl AsRef<std::path::Path>) -> std::io::Result<bool> {
    match metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}
pub fn exists(path: impl AsRef<std::path::Path>) -> bool {
    try_exists(path).unwrap_or(false)
}
pub fn display_path(path: &str) -> String {
    #[cfg(target_os = "android")]
    if is_document_path(std::path::Path::new(path)) {
        return command(serde_json::json!({"op": "display", "path": path})).ok().and_then(|value| value.as_str().map(str::to_owned)).unwrap_or_else(|| "Library folder".to_owned());
    }
    path.to_owned()
}
#[cfg(not(target_os = "android"))]
pub fn is_document_path(_: &std::path::Path) -> bool {
    false
}

#[cfg(target_os = "android")]
pub mod temporary;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
pub use tempfile as temporary;
#[cfg(target_os = "android")]
pub mod walk;
#[cfg(not(any(target_os = "android", target_arch = "wasm32")))]
pub use walkdir as walk;
