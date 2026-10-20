//! Shared physical placement, upload intent, and validated library-relative paths.

use content_address::ContentHash;
use std::error::Error;
use std::fmt;

/// "/book.epub" at the library root, "/a/b/book.epub" below it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RelativeBookPath(String);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelativeBookPathError;

impl fmt::Display for RelativeBookPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("book path must be a non-empty library-relative path without traversal")
    }
}
impl Error for RelativeBookPathError {}

impl RelativeBookPath {
    pub fn parse(value: &str) -> Result<Self, RelativeBookPathError> {
        if value.is_empty() || value.contains(['\\', ':']) || value.chars().any(char::is_control) {
            return Err(RelativeBookPathError);
        }
        let relative = value.strip_prefix('/').unwrap_or(value);
        if relative.is_empty() || relative.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
            return Err(RelativeBookPathError);
        }
        Ok(Self(format!("/{relative}")))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn file_name(&self) -> &str {
        self.0.rsplit_once('/').map_or(self.0.as_str(), |(_, name)| name)
    }
    /// A path inside this one, such as an MP3 folder's track.
    pub fn join(&self, relative: &str) -> Result<Self, RelativeBookPathError> {
        Self::parse(&format!("{}/{relative}", self.0))
    }
    /// The sibling path named `name`.
    pub fn with_file_name(&self, name: &str) -> Result<Self, RelativeBookPathError> {
        let parent = self.0.rsplit_once('/').map_or("", |(parent, _)| parent);
        Self::parse(&format!("{parent}/{name}"))
    }
}
impl fmt::Display for RelativeBookPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BookPlacement {
    pub content_hash: ContentHash,
    pub rel_path: RelativeBookPath,
}
impl BookPlacement {
    pub const fn new(content_hash: ContentHash, rel_path: RelativeBookPath) -> Self {
        Self { content_hash, rel_path }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BookUploadIntent {
    pub id: i64,
    pub content_hash: ContentHash,
    pub size_bytes: u64,
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
impl Error for RelativeDirPathError {}

impl RelativeDirPath {
    pub fn parse(value: &str) -> Result<Self, RelativeDirPathError> {
        if value.is_empty() || value.contains(['\\', ':']) || value.chars().any(char::is_control) {
            return Err(RelativeDirPathError);
        }
        let relative = value.strip_prefix('/').unwrap_or(value);
        if relative.is_empty() || relative.split('/').any(|part| part.is_empty() || part == "." || part == "..") {
            return Err(RelativeDirPathError);
        }
        Ok(Self(relative.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn from_components<'a>(components: impl IntoIterator<Item = &'a str>) -> Result<Self, RelativeDirPathError> {
        Self::parse(&components.into_iter().collect::<Vec<_>>().join("/"))
    }
    pub fn join_file(&self, file_name: &str) -> Result<RelativeBookPath, RelativeBookPathError> {
        RelativeBookPath::parse(&format!("{}/{}", self.0, file_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_reject_windows_prefixes_and_alternate_streams() {
        for path in ["C:/outside/book.epub", "/C:/outside/book.epub", "C:book.epub", "Shelf/book.epub:stream", "Shelf/C:/book.epub", "//server/share/book.epub", r"\\server\share\book.epub", r"\\?\C:\book.epub"] {
            assert!(RelativeBookPath::parse(path).is_err(), "accepted book path: {path}");
            assert!(RelativeDirPath::parse(path).is_err(), "accepted directory path: {path}");
        }
    }

    #[test]
    fn relative_paths_reject_escape_and_normalize_valid_inputs() {
        assert_eq!(RelativeBookPath::parse("Shelf/book.epub").unwrap().as_str(), "/Shelf/book.epub");
        assert!(RelativeBookPath::parse("../book.epub").is_err());
        assert!(RelativeBookPath::parse("/Shelf/../book.epub").is_err());
        assert!(RelativeBookPath::parse("").is_err());
        assert!(RelativeBookPath::parse("Shelf//book.epub").is_err());
        assert!(RelativeBookPath::parse("Shelf/./book.epub").is_err());
        assert!(RelativeBookPath::parse("Shelf/book.epub/").is_err());
        assert!(RelativeBookPath::parse("Shelf\\book.epub").is_err());
        let path = RelativeBookPath::parse("/Shelf/book.epub").unwrap();
        assert_eq!(path.file_name(), "book.epub");
        assert_eq!(path.with_file_name("book (2).epub").unwrap().as_str(), "/Shelf/book (2).epub");
        assert_eq!(RelativeBookPath::parse("book.epub").unwrap().with_file_name("other.epub").unwrap().as_str(), "/other.epub");
        assert!(path.with_file_name("../escape.epub").is_err());
        assert_eq!(RelativeBookPath::parse("Story").unwrap().join("Disc 1/01.mp3").unwrap().as_str(), "/Story/Disc 1/01.mp3");
        assert!(RelativeBookPath::parse("Story").unwrap().join("../01.mp3").is_err());

        assert_eq!(RelativeDirPath::parse("Shelf/Series").unwrap().as_str(), "Shelf/Series");
        assert_eq!(RelativeDirPath::parse("/Shelf/Series").unwrap().as_str(), "Shelf/Series");
        assert!(RelativeDirPath::parse("../Shelf").is_err());
        assert!(RelativeDirPath::parse("Shelf/../Series").is_err());
        assert!(RelativeDirPath::parse("").is_err());
    }
}

/// The resulting file location and fingerprint after staged bytes are published.
#[derive(Clone, Debug)]
pub struct PublishedStagedBook {
    pub name: String,
    pub relative_path: String,
    pub published: bool,
    pub fingerprint: Option<String>,
}
