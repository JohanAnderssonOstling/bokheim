//! Lazy directory walk through the same access boundary as file reads.
use super::{self as fs, FileType, Metadata};
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
pub struct WalkDir {
    root: PathBuf,
    min_depth: usize,
    contents_first: bool,
}
impl WalkDir {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self { root: path.as_ref().to_owned(), min_depth: 0, contents_first: false }
    }
    pub fn contents_first(mut self, value: bool) -> Self {
        self.contents_first = value;
        self
    }
    pub fn follow_links(self, _: bool) -> Self {
        self
    }
    pub fn min_depth(mut self, value: usize) -> Self {
        self.min_depth = value;
        self
    }
}
pub struct IntoIter {
    pending: Vec<(PathBuf, usize, bool, Option<Metadata>)>,
    descend: Option<(PathBuf, usize)>,
    min_depth: usize,
    contents_first: bool,
}
pub struct DirEntry {
    path: PathBuf,
    depth: usize,
    metadata: Metadata,
}
impl DirEntry {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn into_path(self) -> PathBuf {
        self.path
    }
    pub fn file_name(&self) -> &OsStr {
        self.path.file_name().unwrap_or_default()
    }
    pub fn depth(&self) -> usize {
        self.depth
    }
    pub fn file_type(&self) -> FileType {
        self.metadata.file_type()
    }
    pub fn metadata(&self) -> Result<Metadata, Error> {
        Ok(self.metadata.clone())
    }
}
#[derive(Debug)]
pub struct Error(io::Error);
impl Error {
    pub fn into_io_error(self) -> Option<io::Error> {
        Some(self.0)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Error {}
impl IntoIter {
    pub fn skip_current_dir(&mut self) {
        self.descend = None;
    }
}
impl IntoIterator for WalkDir {
    type Item = Result<DirEntry, Error>;
    type IntoIter = IntoIter;
    fn into_iter(self) -> IntoIter {
        IntoIter { pending: vec![(self.root, 0, false, None)], descend: None, min_depth: self.min_depth, contents_first: self.contents_first }
    }
}
impl Iterator for IntoIter {
    type Item = Result<DirEntry, Error>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some((path, depth)) = self.descend.take() {
                match fs::read_dir(path) {
                    Ok(children) => {
                        let children: io::Result<Vec<_>> = children.map(|entry| entry.and_then(|entry| Ok((entry.path(), depth + 1, false, Some(entry.metadata()?))))).collect();
                        match children {
                            Ok(children) => self.pending.extend(children.into_iter().rev()),
                            Err(error) => return Some(Err(Error(error))),
                        }
                    }
                    Err(error) => return Some(Err(Error(error))),
                }
            }
            let (path, depth, visited, observed) = self.pending.pop()?;
            let metadata = match observed.map(Ok).unwrap_or_else(|| fs::symlink_metadata(&path)) {
                Ok(value) => value,
                Err(error) => return Some(Err(Error(error))),
            };
            if metadata.is_dir() && !visited {
                self.descend = Some((path.clone(), depth));
                if self.contents_first {
                    self.pending.push((path, depth, true, Some(metadata)));
                    continue;
                }
            }
            if depth >= self.min_depth {
                return Some(Ok(DirEntry { path, depth, metadata }));
            }
        }
    }
}
