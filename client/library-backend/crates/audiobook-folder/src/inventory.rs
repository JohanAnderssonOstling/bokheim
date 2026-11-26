//! Temporary directory observations; revision checks build a fresh inventory.

use crate::local_names::{self, PathMapping};
use audiobook_archive::{Cover, Cue, MAX_COVER_BYTES, MAX_CUE_BYTES, MAX_NFO_BYTES, Nfo, Track, valid_sidecar_name};
use client_platform_runtime::filesystem::{self as fs, DirEntry};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

pub struct FolderInventory {
    folder: PathBuf,
    directories: HashMap<PathBuf, Vec<DirEntry>>,
    mapping: Option<Option<PathMapping>>,
}

impl FolderInventory {
    pub fn read(folder: &Path) -> io::Result<Self> {
        let entries = fs::read_dir(folder)?.collect::<io::Result<Vec<_>>>()?;
        Ok(Self::from_directories(folder, HashMap::from([(folder.to_owned(), entries)])))
    }

    /// Reuse listings from a traversal. The caller must supply the root listing.
    pub fn from_directories(folder: &Path, directories: HashMap<PathBuf, Vec<DirEntry>>) -> Self {
        Self { folder: folder.to_owned(), directories, mapping: None }
    }

    pub fn into_directories(self) -> HashMap<PathBuf, Vec<DirEntry>> {
        self.directories
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// List a directory at most once during this operation.
    pub fn entries(&mut self, directory: &Path) -> io::Result<&[DirEntry]> {
        use std::collections::hash_map::Entry;
        Ok(match self.directories.entry(directory.to_owned()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(fs::read_dir(directory)?.collect::<io::Result<Vec<_>>>()?),
        })
    }

    pub fn has_mapping(&self) -> bool {
        self.directories[&self.folder].iter().any(|entry| entry.file_name() == local_names::PATH_MAPPING_FILE)
    }

    pub(crate) fn mapping(&mut self) -> io::Result<Option<&PathMapping>> {
        if self.mapping.is_none() {
            self.mapping = Some(if self.has_mapping() { local_names::load(&self.folder)? } else { None });
        }
        Ok(self.mapping.as_ref().unwrap().as_ref())
    }

    pub(crate) fn apply_track_mapping(&mut self, tracks: &mut Vec<Track>) -> io::Result<()> {
        self.mapping()?;
        let mapping = self.mapping.take().flatten();
        let result = if let Some(mapping) = &mapping {
            let folder = self.folder.clone();
            mapping.apply_tracks(&folder, tracks, |local| self.regular_mapped_track(local))
        } else {
            Ok(())
        };
        self.mapping = Some(mapping);
        result
    }

    fn regular_mapped_track(&mut self, local: &str) -> io::Result<bool> {
        let mut directory = self.folder.clone();
        let mut components = local.split('/').peekable();
        while let Some(component) = components.next() {
            let entries = match self.entries(&directory) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(error),
            };
            let kind = entries.iter().find(|entry| entry.file_name() == component).map(|entry| entry.file_type()).transpose()?;
            let Some(kind) = kind else {
                // Preserve filesystem-specific name matching (for example, case
                // changes on a case-insensitive filesystem) on the uncommon miss.
                return local_names::regular_mapped_track(&self.folder, local);
            };
            if kind.is_symlink() || !(if components.peek().is_some() { kind.is_dir() } else { kind.is_file() }) {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "mapped audiobook track must use regular files and directories"));
            }
            directory.push(component);
        }
        Ok(true)
    }

    pub(crate) fn sidecar(&mut self, accepts: impl Fn(&str) -> bool, maximum: u64) -> io::Result<Option<Nfo>> {
        self.mapping()?;
        let mut found = None;
        for entry in &self.directories[&self.folder] {
            let Ok(name) = entry.file_name().into_string() else { continue };
            if name.starts_with('.') || !accepts(&name) || !entry.file_type()?.is_file() {
                continue;
            }
            if !valid_sidecar_name(&name) || entry.metadata()?.len() > maximum {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid or oversized audiobook sidecar"));
            }
            if found.replace(Nfo { name, path: entry.path() }).is_some() {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "audiobook has multiple matching sidecars"));
            }
        }
        if let Some(sidecar) = &mut found
            && let Some(mapping) = self.mapping()?
        {
            sidecar.name = mapping.logical_name(&sidecar.name).to_owned();
        }
        Ok(found)
    }

    pub fn nfo(&mut self) -> io::Result<Option<Nfo>> {
        self.sidecar(|name| name.to_ascii_lowercase().ends_with(".nfo"), MAX_NFO_BYTES)
    }

    pub fn cue(&mut self) -> io::Result<Option<Cue>> {
        self.sidecar(|name| name.to_ascii_lowercase().ends_with(".cue"), MAX_CUE_BYTES)
    }

    pub fn cover(&mut self) -> io::Result<Option<Cover>> {
        for stem in ["cover", "folder"] {
            for extension in ["jpg", "jpeg", "png", "webp"] {
                if let Some(cover) = self.sidecar(|name| name.eq_ignore_ascii_case(&format!("{stem}.{extension}")), MAX_COVER_BYTES)? {
                    return Ok(Some(cover));
                }
            }
        }
        Ok(None)
    }

    /// Include every physical file, including unrelated and hidden files, without following symlinks.
    pub fn physical_size_bytes(&mut self) -> io::Result<u64> {
        let mut directories = vec![self.folder.clone()];
        let mut total = 0u64;
        while let Some(directory) = directories.pop() {
            for entry in self.entries(&directory)? {
                let kind = entry.file_type()?;
                if kind.is_dir() {
                    directories.push(entry.path());
                } else if kind.is_file() {
                    total = total.checked_add(entry.metadata()?.len()).ok_or_else(|| io::Error::other("folder size overflow"))?;
                }
            }
        }
        Ok(total)
    }
}
