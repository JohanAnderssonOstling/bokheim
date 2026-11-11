//! String-error adapters for the shared durable update file operations.
use crate::UpdateError;
use std::path::Path;

pub fn parent_directory(path: &Path) -> Result<&Path, UpdateError> {
    update_files::parent_directory(path).map_err(UpdateError::operation)
}
pub fn sync_file(path: &Path) -> Result<(), UpdateError> {
    update_files::sync_file(path).map_err(UpdateError::operation)
}
pub fn sync_dir(path: &Path) -> Result<(), UpdateError> {
    update_files::sync_dir(path).map_err(UpdateError::operation)
}
pub fn replace(source: &Path, destination: &Path) -> Result<(), UpdateError> {
    update_files::replace(source, destination).map_err(UpdateError::operation)
}
pub fn remove(path: &Path) -> Result<(), UpdateError> {
    update_files::remove(path).map_err(UpdateError::operation)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn replaces_existing_file_and_preserves_it_on_missing_source() {
        let dir = tempfile::tempdir().unwrap();
        let next = dir.path().join("next");
        let live = dir.path().join("live");
        fs::write(&live, b"old").unwrap();
        fs::write(&next, b"new").unwrap();
        sync_file(&next).unwrap();
        replace(&next, &live).unwrap();
        assert_eq!(fs::read(&live).unwrap(), b"new");
        assert!(replace(&next, &live).is_err());
        assert_eq!(fs::read(&live).unwrap(), b"new");
    }

    #[cfg(windows)]
    #[test]
    fn locked_destination_can_be_retried_after_owner_exits() {
        use std::fs::OpenOptions;
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("Bokheim.exe");
        let next = dir.path().join("candidate.exe");
        fs::write(&live, b"old").unwrap();
        fs::write(&next, b"new").unwrap();
        let owner = OpenOptions::new().read(true).share_mode(0).open(&live).unwrap();
        assert!(replace(&next, &live).is_err());
        assert!(next.exists());
        drop(owner);
        assert_eq!(fs::read(&live).unwrap(), b"old");
        replace(&next, &live).unwrap();
        assert_eq!(fs::read(&live).unwrap(), b"new");
    }
}
