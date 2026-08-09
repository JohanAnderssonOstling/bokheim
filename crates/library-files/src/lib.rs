//! Enforces ownership boundaries for filesystem operations inside a library.

use domain::{RelativeBookPath, RelativeDirPath};
use identifiers::{ContentHash, DirId};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MANAGED_DIRECTORY_MARKER: &str = ".biblos_uuid";
const TEMPORARY_FILE_PREFIX: &str = ".bokheim";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LibraryRoot(PathBuf);

#[derive(Debug)]
pub enum VerifiedBookCopyError {
    InvalidSource(std::io::Error),
    InvalidDestination(std::io::Error),
    HashMismatch { expected: String, actual: String },
}

impl VerifiedBookCopyError {
    pub const fn invalidates_source(&self) -> bool {
        matches!(self, Self::InvalidSource(_) | Self::HashMismatch { .. })
    }
}

impl fmt::Display for VerifiedBookCopyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSource(error) => write!(formatter, "invalid local blob source: {error}"),
            Self::InvalidDestination(error) => write!(formatter, "invalid local blob destination: {error}"),
            Self::HashMismatch { expected, actual } => write!(formatter, "local blob hash mismatch: expected {expected}, got {actual}"),
        }
    }
}

impl std::error::Error for VerifiedBookCopyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidSource(error) | Self::InvalidDestination(error) => Some(error),
            Self::HashMismatch { .. } => None,
        }
    }
}

impl LibraryRoot {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        let canonical = path.canonicalize()?;
        if !canonical.is_dir() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("library root is not a directory: {}", path.display())));
        }
        if canonical.to_str().is_none() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "library root is not valid UTF-8"));
        }
        Ok(Self(canonical))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn resolve_book(&self, path: &RelativeBookPath) -> PathBuf {
        path.resolve_under(&self.0)
    }

    pub fn resolve_dir(&self, path: &RelativeDirPath) -> PathBuf {
        path.resolve_under(&self.0)
    }

    pub fn existing_file(&self, path: &Path) -> std::io::Result<PathBuf> {
        let canonical = path.canonicalize()?;
        if !canonical.starts_with(&self.0) || !canonical.is_file() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("file is not contained by the library root: {}", path.display())));
        }
        Ok(canonical)
    }

    pub fn copy_verified_book(&self, source_hint: &Path, destination: &RelativeBookPath, expected_hash: &ContentHash) -> Result<PathBuf, VerifiedBookCopyError> {
        let source = self.existing_file(source_hint).map_err(VerifiedBookCopyError::InvalidSource)?;
        let mut source_file = File::open(&source).map_err(VerifiedBookCopyError::InvalidSource)?;
        let unresolved_destination = self.resolve_book(destination);
        let destination_parent = unresolved_destination.parent().ok_or_else(|| VerifiedBookCopyError::InvalidDestination(std::io::Error::new(std::io::ErrorKind::InvalidInput, "book destination has no parent")))?;
        fs::create_dir_all(destination_parent).map_err(VerifiedBookCopyError::InvalidDestination)?;
        let destination_parent = destination_parent.canonicalize().map_err(VerifiedBookCopyError::InvalidDestination)?;
        if !destination_parent.starts_with(&self.0) {
            return Err(VerifiedBookCopyError::InvalidDestination(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("book destination escapes the library root: {}", unresolved_destination.display()))));
        }
        let file_name = unresolved_destination.file_name().ok_or_else(|| VerifiedBookCopyError::InvalidDestination(std::io::Error::new(std::io::ErrorKind::InvalidInput, "book destination has no file name")))?;
        let destination = destination_parent.join(file_name);
        let temporary = destination_parent.join(format!("{TEMPORARY_FILE_PREFIX}-copy-{}.tmp", Uuid::new_v4()));
        let mut temporary_file = OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(VerifiedBookCopyError::InvalidDestination)?;
        let copy_result = (|| {
            let mut hasher = blake3::Hasher::new();
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = source_file.read(&mut buffer).map_err(VerifiedBookCopyError::InvalidSource)?;
                if read == 0 {
                    break;
                }
                hasher.update(&buffer[..read]);
                temporary_file.write_all(&buffer[..read]).map_err(VerifiedBookCopyError::InvalidDestination)?;
            }
            temporary_file.flush().map_err(VerifiedBookCopyError::InvalidDestination)?;
            let actual = hasher.finalize().to_hex().to_string();
            if actual != expected_hash.as_str() {
                return Err(VerifiedBookCopyError::HashMismatch { expected: expected_hash.as_str().to_owned(), actual });
            }
            temporary_file.sync_all().map_err(VerifiedBookCopyError::InvalidDestination)
        })();
        drop(temporary_file);
        if let Err(error) = copy_result {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        if destination.exists() {
            let _ = fs::remove_file(&temporary);
            return Err(VerifiedBookCopyError::InvalidDestination(std::io::Error::new(std::io::ErrorKind::AlreadyExists, format!("book destination appeared during verified copy: {}", destination.display()))));
        }
        if let Err(error) = fs::rename(&temporary, &destination) {
            let _ = fs::remove_file(&temporary);
            return Err(VerifiedBookCopyError::InvalidDestination(error));
        }
        Ok(destination)
    }
}

fn verified_directory(root: &LibraryRoot, directory: &Path, dir_id: &DirId) -> std::io::Result<()> {
    let canonical = directory.canonicalize()?;
    if !canonical.starts_with(root.as_path()) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "managed path escaped the library root"));
    }
    if dir_id.as_str() == "__root__" {
        return if canonical == root.as_path() { Ok(()) } else { Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "root placement is not directly inside the library root")) };
    }
    let marker = fs::read_to_string(canonical.join(MANAGED_DIRECTORY_MARKER))?;
    if marker.trim() != dir_id.as_str() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "managed-directory marker does not match"));
    }
    Ok(())
}

pub fn remove_managed_book_file(root: &LibraryRoot, path: &Path, dir_id: &DirId) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "book path has no parent"))?;
    verified_directory(root, parent, dir_id)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn remove_managed_dir_if_empty(root: &LibraryRoot, path: &Path, dir_id: &DirId) -> std::io::Result<()> {
    verified_directory(root, path, dir_id)?;
    let marker = path.join(MANAGED_DIRECTORY_MARKER);
    fs::remove_file(&marker)?;
    match fs::remove_dir(path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::write(marker, dir_id.as_str());
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_rejects_files_outside_its_boundary() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let library_root = LibraryRoot::open(root.path()).unwrap();
        assert!(library_root.existing_file(outside.path()).is_err());
    }

    #[test]
    fn verified_copy_hashes_while_installing() {
        let root = tempfile::tempdir().unwrap();
        let library_root = LibraryRoot::open(root.path()).unwrap();
        let source = root.path().join("source.epub");
        fs::write(&source, b"verified book bytes").unwrap();
        let hash = ContentHash::new(&blake3::hash(b"verified book bytes").to_hex().as_str());
        let destination = RelativeBookPath::parse("Destination/book.epub").unwrap();

        let installed = library_root.copy_verified_book(&source, &destination, &hash).unwrap();

        assert_eq!(fs::read(installed).unwrap(), b"verified book bytes");
    }

    #[test]
    fn verified_copy_removes_temporary_file_after_hash_mismatch() {
        let root = tempfile::tempdir().unwrap();
        let library_root = LibraryRoot::open(root.path()).unwrap();
        let source = root.path().join("source.epub");
        fs::write(&source, b"wrong bytes").unwrap();
        let hash = ContentHash::new(&blake3::hash(b"expected bytes").to_hex().as_str());
        let destination = RelativeBookPath::parse("Destination/book.epub").unwrap();

        let error = library_root.copy_verified_book(&source, &destination, &hash).unwrap_err();

        assert!(matches!(error, VerifiedBookCopyError::HashMismatch { .. }));
        assert!(!library_root.resolve_book(&destination).exists());
        assert!(fs::read_dir(root.path().join("Destination")).unwrap().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn verified_copy_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let library_root = LibraryRoot::open(root.path()).unwrap();
        let outside_source = outside.path().join("book.epub");
        fs::write(&outside_source, b"book bytes").unwrap();
        let source_link = root.path().join("source.epub");
        symlink(&outside_source, &source_link).unwrap();
        let hash = ContentHash::new(&blake3::hash(b"book bytes").to_hex().as_str());

        let error = library_root.copy_verified_book(&source_link, &RelativeBookPath::parse("Safe/book.epub").unwrap(), &hash).unwrap_err();

        assert!(matches!(error, VerifiedBookCopyError::InvalidSource(_)));
    }
}
