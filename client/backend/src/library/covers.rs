//! Generates and atomically publishes UI and synchronized book covers.

use crate::ContentHash;
use crate::app::BackendContext;
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};

pub(crate) fn generate_thumbnail_for_sync(context: &BackendContext, library_root: &Path, book_path: &Path, content_hash: &ContentHash) -> Result<bool, Box<dyn std::error::Error>> {
    let thumbnail_path = context.thumbnails_dir()?.join(format!("{content_hash}.jpg"));
    let sync_path = crate::sync::transfer::thumbnail_path_from_content_hash(library_root, content_hash);
    let publish_thumbnail = thumbnail_path_needs_publish(&thumbnail_path)?;
    let publish_sync = thumbnail_path_needs_publish(&sync_path)?;
    if !publish_thumbnail && !publish_sync {
        return Ok(true);
    }

    let Some(thumbnail) = thumbnail::generate_thumbnail(book_path)? else {
        return Ok(false);
    };

    if publish_thumbnail {
        atomic_publish_bytes(&thumbnail_path, &thumbnail)?;
    }
    if publish_sync {
        atomic_publish_bytes(&sync_path, &thumbnail)?;
    }
    Ok(true)
}

/// Returns true when either crash-safe publication target still needs to be
/// generated. The scanner calls this for fingerprint-known books so an
/// in-memory thumbnail job lost during shutdown is reconstructed on restart.
pub(crate) fn thumbnail_needs_repair(context: &BackendContext, library_root: &Path, content_hash: &ContentHash) -> std::io::Result<bool> {
    let thumbnail_path = context.thumbnails_dir()?.join(format!("{content_hash}.jpg"));
    let sync_path = crate::sync::transfer::thumbnail_path_from_content_hash(library_root, content_hash);
    thumbnail_targets_need_repair(&thumbnail_path, &sync_path)
}

fn thumbnail_targets_need_repair(thumbnail_path: &Path, sync_path: &Path) -> std::io::Result<bool> {
    Ok(thumbnail_path_needs_publish(thumbnail_path)? || thumbnail_path_needs_publish(sync_path)?)
}

fn thumbnail_path_needs_publish(path: &Path) -> std::io::Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(!metadata.is_file() || metadata.len() == 0),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

fn atomic_publish_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "thumbnail target has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = PathBuf::from(format!("{}.tmp-{}", path.display(), uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        std::io::Write::write_all(&mut file, bytes)?;
        file.sync_all()?;
        match fs::hard_link(&temporary, path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => {
                if let Err(rename_error) = fs::rename(&temporary, path) {
                    if rename_error.kind() == std::io::ErrorKind::AlreadyExists {
                        fs::remove_file(path)?;
                        fs::rename(&temporary, path)?;
                    } else {
                        fs::copy(&temporary, path)?;
                    }
                }
            }
        }
        let _ = fs::remove_file(&temporary);
        File::open(parent)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn missing_or_empty_thumbnail_target_is_repairable_after_restart() {
        let temporary = tempdir().unwrap();
        let ui_path = temporary.path().join("ui.jpg");
        let sync_path = temporary.path().join("sync.jpg");
        assert!(thumbnail_targets_need_repair(&ui_path, &sync_path).unwrap());

        fs::write(&ui_path, b"thumbnail").unwrap();
        assert!(thumbnail_targets_need_repair(&ui_path, &sync_path).unwrap());

        fs::write(&sync_path, b"thumbnail").unwrap();
        assert!(!thumbnail_targets_need_repair(&ui_path, &sync_path).unwrap());

        fs::write(&ui_path, []).unwrap();
        assert!(thumbnail_targets_need_repair(&ui_path, &sync_path).unwrap());
    }
}
