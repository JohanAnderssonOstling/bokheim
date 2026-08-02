use crate::{BookId, ContentHash};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn normalized_relative_path(value: &str) -> Result<String, RelativePathError> {
    if value.is_empty() || value.contains('\\') || value.chars().any(char::is_control) {
        return Err(RelativePathError);
    }
    let relative = value.strip_prefix('/').unwrap_or(value);
    if relative.is_empty() || relative.split('/').any(|component| component.is_empty() || component == "." || component == "..") {
        return Err(RelativePathError);
    }
    Ok(relative.to_owned())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RelativePathError;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RelativeBookPath(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelativeBookPathError;

impl fmt::Display for RelativeBookPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("book path must be a non-empty library-relative path without traversal")
    }
}

impl std::error::Error for RelativeBookPathError {}

impl RelativeBookPath {
    pub fn parse(value: &str) -> Result<Self, RelativeBookPathError> {
        let relative = normalized_relative_path(value).map_err(|_| RelativeBookPathError)?;
        Ok(Self(format!("/{relative}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn resolve_under(&self, root: &Path) -> std::path::PathBuf {
        root.join(self.0.trim_start_matches('/'))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RelativeDirPath(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelativeDirPathError;

impl fmt::Display for RelativeDirPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("directory path must be a non-empty library-relative path without traversal")
    }
}

impl std::error::Error for RelativeDirPathError {}

impl RelativeDirPath {
    pub fn parse(value: &str) -> Result<Self, RelativeDirPathError> {
        normalized_relative_path(value).map(Self).map_err(|_| RelativeDirPathError)
    }

    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn resolve_under(&self, root: &Path) -> std::path::PathBuf {
        root.join(&self.0)
    }

    pub(crate) fn from_components<'a>(components: impl IntoIterator<Item = &'a str>) -> Result<Self, RelativeDirPathError> {
        Self::parse(&components.into_iter().collect::<Vec<_>>().join("/"))
    }

    pub(crate) fn join_file(&self, file_name: &sync_common::FileName) -> Result<RelativeBookPath, RelativeBookPathError> {
        RelativeBookPath::parse(&format!("{}/{}", self.0, file_name.as_str()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LibraryRoot(PathBuf);

#[derive(Debug)]
pub(crate) enum VerifiedBookCopyError {
    InvalidSource(std::io::Error),
    InvalidDestination(std::io::Error),
    HashMismatch { expected: String, actual: String },
}

impl VerifiedBookCopyError {
    pub(crate) const fn invalidates_source(&self) -> bool {
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
    pub(crate) fn open(path: &Path) -> std::io::Result<Self> {
        let canonical = path.canonicalize()?;
        if !canonical.is_dir() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("library root is not a directory: {}", path.display())));
        }
        if canonical.to_str().is_none() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "library root is not valid UTF-8"));
        }
        Ok(Self(canonical))
    }

    pub(crate) fn as_path(&self) -> &Path {
        &self.0
    }

    pub(crate) fn resolve_book(&self, path: &RelativeBookPath) -> PathBuf {
        path.resolve_under(&self.0)
    }

    pub(crate) fn resolve_dir(&self, path: &RelativeDirPath) -> PathBuf {
        path.resolve_under(&self.0)
    }

    pub(crate) fn existing_file(&self, path: &Path) -> std::io::Result<PathBuf> {
        let canonical = path.canonicalize()?;
        if !canonical.starts_with(&self.0) || !canonical.is_file() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("file is not contained by the library root: {}", path.display())));
        }
        Ok(canonical)
    }

    pub(crate) fn copy_verified_book(&self, source_hint: &Path, destination: &RelativeBookPath, expected_hash: &ContentHash) -> Result<PathBuf, VerifiedBookCopyError> {
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
        let temporary = destination_parent.join(format!("{}-copy-{}.tmp", crate::APP_HIDDEN_DIR, Uuid::new_v4()));
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
            let actual = hasher.finalize().to_hex().to_string();
            if actual != expected_hash.as_str() {
                return Err(VerifiedBookCopyError::HashMismatch { expected: expected_hash.as_str().to_owned(), actual });
            }
            temporary_file.flush().map_err(VerifiedBookCopyError::InvalidDestination)?;
            temporary_file.sync_all().map_err(VerifiedBookCopyError::InvalidDestination)?;
            Ok(())
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

impl fmt::Display for RelativeBookPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BookPlacement {
    pub book_id: BookId,
    pub rel_path: RelativeBookPath,
}

impl BookPlacement {
    pub const fn new(book_id: BookId, rel_path: RelativeBookPath) -> Self {
        Self { book_id, rel_path }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_book_paths_reject_escape_and_normalize_the_leading_slash() {
        assert_eq!(RelativeBookPath::parse("Shelf/book.epub").unwrap().as_str(), "/Shelf/book.epub");
        assert!(RelativeBookPath::parse("../book.epub").is_err());
        assert!(RelativeBookPath::parse("/Shelf/../book.epub").is_err());
        assert!(RelativeBookPath::parse("").is_err());
        assert!(RelativeBookPath::parse("Shelf//book.epub").is_err());
        assert!(RelativeBookPath::parse("Shelf/./book.epub").is_err());
        assert!(RelativeBookPath::parse("Shelf/book.epub/").is_err());
        assert!(RelativeBookPath::parse("Shelf\\book.epub").is_err());
    }

    #[test]
    fn relative_directory_paths_reject_escape_and_normalize_the_leading_slash() {
        assert_eq!(RelativeDirPath::parse("Shelf/Series").unwrap().as_path(), Path::new("Shelf/Series"));
        assert_eq!(RelativeDirPath::parse("/Shelf/Series").unwrap().as_path(), Path::new("Shelf/Series"));
        assert!(RelativeDirPath::parse("../Shelf").is_err());
        assert!(RelativeDirPath::parse("Shelf/../Series").is_err());
        assert!(RelativeDirPath::parse("").is_err());
    }

    #[test]
    fn library_root_canonicalizes_before_resolving_typed_paths() {
        let root = tempfile::tempdir().unwrap();
        let library_root = LibraryRoot::open(root.path()).unwrap();
        let book = RelativeBookPath::parse("Shelf/book.epub").unwrap();
        let dir = RelativeDirPath::parse("Shelf").unwrap();
        assert_eq!(library_root.resolve_book(&book), root.path().canonicalize().unwrap().join("Shelf/book.epub"));
        assert_eq!(library_root.resolve_dir(&dir), root.path().canonicalize().unwrap().join("Shelf"));
    }

    #[test]
    fn library_root_rejects_files_outside_its_ownership_boundary() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let library_root = LibraryRoot::open(root.path()).unwrap();
        assert!(library_root.existing_file(outside.path()).is_err());

        let inside = root.path().join("book.epub");
        std::fs::write(&inside, b"book").unwrap();
        assert_eq!(library_root.existing_file(&inside).unwrap(), inside.canonicalize().unwrap());
    }

    #[test]
    fn verified_book_copy_hashes_while_installing_atomically() {
        let root = tempfile::tempdir().unwrap();
        let library_root = LibraryRoot::open(root.path()).unwrap();
        let source = root.path().join("Source/book.epub");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"verified book bytes").unwrap();
        let expected_hash = ContentHash::parse(blake3::hash(b"verified book bytes").to_hex().as_str()).unwrap();
        let destination = RelativeBookPath::parse("Destination/book.epub").unwrap();

        let installed = library_root.copy_verified_book(&source, &destination, &expected_hash).unwrap();

        assert_eq!(installed, root.path().canonicalize().unwrap().join("Destination/book.epub"));
        assert_eq!(fs::read(installed).unwrap(), b"verified book bytes");
        let copy_prefix = format!("{}-copy-", crate::APP_HIDDEN_DIR);
        assert!(fs::read_dir(root.path().join("Destination")).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with(&copy_prefix)));
    }

    #[test]
    fn verified_book_copy_removes_temporary_file_on_hash_mismatch() {
        let root = tempfile::tempdir().unwrap();
        let library_root = LibraryRoot::open(root.path()).unwrap();
        let source = root.path().join("book.epub");
        fs::write(&source, b"wrong bytes").unwrap();
        let expected_hash = ContentHash::parse(blake3::hash(b"expected bytes").to_hex().as_str()).unwrap();
        let destination = RelativeBookPath::parse("Destination/book.epub").unwrap();

        let error = library_root.copy_verified_book(&source, &destination, &expected_hash).unwrap_err();

        assert!(matches!(&error, VerifiedBookCopyError::HashMismatch { .. }));
        assert!(error.invalidates_source());
        assert!(!library_root.resolve_book(&destination).exists());
        assert!(fs::read_dir(root.path().join("Destination")).unwrap().next().is_none());
    }

    #[test]
    fn verified_book_copy_rejects_source_and_destination_symlink_escapes() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let library_root = LibraryRoot::open(root.path()).unwrap();
        let outside_source = outside.path().join("book.epub");
        fs::write(&outside_source, b"book bytes").unwrap();
        let expected_hash = ContentHash::parse(blake3::hash(b"book bytes").to_hex().as_str()).unwrap();
        let source_link = root.path().join("source.epub");
        symlink(&outside_source, &source_link).unwrap();
        let safe_destination = RelativeBookPath::parse("Safe/book.epub").unwrap();
        let source_error = library_root.copy_verified_book(&source_link, &safe_destination, &expected_hash).unwrap_err();
        assert!(matches!(source_error, VerifiedBookCopyError::InvalidSource(_)));

        let inside_source = root.path().join("inside.epub");
        fs::write(&inside_source, b"book bytes").unwrap();
        symlink(outside.path(), root.path().join("Escape")).unwrap();
        let escaping_destination = RelativeBookPath::parse("Escape/copied.epub").unwrap();
        let destination_error = library_root.copy_verified_book(&inside_source, &escaping_destination, &expected_hash).unwrap_err();
        assert!(matches!(destination_error, VerifiedBookCopyError::InvalidDestination(_)));
        assert!(!outside.path().join("copied.epub").exists());
    }
}
