//! Delete only source-owned cache files before dropping their database references.
use crate::{CatalogServiceError, CatalogSourceId, CatalogStore};
use std::{collections::HashSet, io, path::Path};

pub(crate) fn purge_files(store: &CatalogStore, source: &CatalogSourceId, acquisitions: &Path, covers: &Path) -> Result<(), CatalogServiceError> {
    let (paths, urls, retained) = store.source_file_references(source).map_err(crate::store_error)?;
    let count = purge(&paths, &urls, &retained, acquisitions, covers).map_err(|e| CatalogServiceError(format!("could not purge cached files for {}: {e}",source.as_str())))?;
    tracing::info!(source = source.as_str(), files = count, "purged removed source cache files");
    Ok(())
}

fn regular_bytes(path: &Path) -> io::Result<Option<Vec<u8>>> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
        Ok(metadata) if metadata.is_file() => std::fs::read(path).map(Some),
        Ok(_) => Err(io::Error::other("refusing a non-regular cache file")),
    }
}

fn remove_regular(path: &Path) -> io::Result<usize> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(e) => Err(e),
        Ok(metadata) if metadata.is_file() => std::fs::remove_file(path).map(|_| 1),
        Ok(_) => Err(io::Error::other("refusing to remove a non-regular cache file")),
    }
}

fn purge(paths: &[String], urls: &[String], retained: &[String], acquisitions: &Path, covers: &Path) -> io::Result<usize> {
    let mut count = 0;
    if acquisitions.exists() {
        let root = acquisitions.canonicalize()?;
        for path in paths {
            let path = Path::new(path);
            let parent = path.parent().ok_or_else(|| io::Error::other("staged path has no parent"))?;
            let name = path.file_name().and_then(|n| n.to_str()).ok_or_else(|| io::Error::other("invalid staged filename"))?;
            if !name.starts_with("opds-") || parent.canonicalize()? != root {
                return Err(io::Error::other("refusing to purge a staged path outside the acquisition directory"));
            }
            count += remove_regular(&root.join(name))?;
        }
    }
    if !covers.exists() {
        return Ok(count);
    }
    let root = covers.canonicalize()?;
    let retained = retained.iter().collect::<HashSet<_>>();
    let mut digests = HashSet::new();
    let mut owned_files = HashSet::new();
    for url in urls {
        if retained.contains(url) {
            continue;
        }
        if let Some(digest) = catalog_core::catalog_cover_asset_digest(url) {
            digests.insert(digest.to_owned());
        }
        let path = root.join(format!("{}.jpg", blake3::hash(url.as_bytes()).to_hex()));
        if let Some(bytes) = regular_bytes(&path)? {
            digests.insert(blake3::hash(&bytes).to_hex().to_string());
            owned_files.insert(path);
        }
    }
    // Content-addressed covers may be shared across sources. Keep any digest
    // still referenced directly or by a remaining URL-keyed cache file.
    for url in retained {
        if let Some(digest) = catalog_core::catalog_cover_asset_digest(url) {
            digests.remove(digest);
        }
    }
    for entry in std::fs::read_dir(&root)? {
        if digests.is_empty() {
            break;
        }
        let entry = entry?;
        if owned_files.contains(&entry.path()) {
            continue;
        }
        let name = entry.file_name();
        let Some(stem) = name.to_str().and_then(|n| n.strip_suffix(".jpg")) else { continue };
        if stem.len() != 64 || !stem.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        if let Some(bytes) = regular_bytes(&entry.path())? {
            digests.remove(blake3::hash(&bytes).to_hex().as_str());
        }
    }
    let assets = root.join("assets");
    if !digests.is_empty() && assets.exists() {
        if assets.canonicalize()? != assets {
            return Err(io::Error::other("refusing a redirected cover asset directory"));
        }
        for digest in digests {
            count += remove_regular(&assets.join(format!("{digest}.jpg")))?;
        }
    }
    // Remove URL-keyed originals last, so a crash before asset cleanup leaves
    // enough information for the next reconciliation to retry safely.
    for path in owned_files {
        count += remove_regular(&path)?;
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purge_removes_owned_files_but_preserves_shared_assets_and_other_sources() {
        let temp = tempfile::tempdir().unwrap();
        let acquisitions = temp.path().join("acquisitions");
        let covers = temp.path().join("covers");
        std::fs::create_dir_all(&acquisitions).unwrap();
        std::fs::create_dir_all(covers.join("assets")).unwrap();
        let staged = acquisitions.join("opds-owned.epub");
        std::fs::write(&staged, b"book").unwrap();
        let urls = vec!["https://removed/one".to_owned(), "https://removed/shared".to_owned()];
        let retained = vec!["https://kept/shared".to_owned()];
        for (url, bytes) in [(&urls[0], b"unique".as_slice()), (&urls[1], b"shared".as_slice()), (&retained[0], b"shared".as_slice())] {
            std::fs::write(covers.join(format!("{}.jpg", blake3::hash(url.as_bytes()).to_hex())), bytes).unwrap();
            std::fs::write(covers.join("assets").join(format!("{}.jpg", blake3::hash(bytes).to_hex())), bytes).unwrap();
        }
        assert_eq!(purge(&[staged.to_string_lossy().into_owned()], &urls, &retained, &acquisitions, &covers).unwrap(), 4);
        assert!(!staged.exists());
        assert!(covers.join("assets").join(format!("{}.jpg", blake3::hash(b"shared").to_hex())).exists());
        assert!(!covers.join("assets").join(format!("{}.jpg", blake3::hash(b"unique").to_hex())).exists());
        assert_eq!(purge(&[], &urls, &retained, &acquisitions, &covers).unwrap(), 0);
        let outside = temp.path().join("opds-outside.epub");
        std::fs::write(&outside, b"untouched").unwrap();
        assert!(purge(&[outside.to_string_lossy().into_owned()], &[], &[], &acquisitions, &covers).is_err());
        assert!(outside.exists());
    }
}
