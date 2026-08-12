//! Persists the device-local catalogue of library roots and their manifests.

use csv::{Reader, WriterBuilder};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use sync_common::LibraryId;
use uuid::Uuid;

pub const LIBRARY_MANIFEST_FILENAME: &str = ".bokheim-library.json";
const MANIFEST_FORMAT: &str = "bokheim-library";
const MANIFEST_VERSION: u32 = 1;
const METADATA_DIRECTORY: &str = ".bokheim";
const DETACHED_LIBRARIES_FILENAME: &str = "detached-libraries.json";

pub type RegistryResult<T> = Result<T, Box<dyn std::error::Error>>;

fn canonical_library_path(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = path.canonicalize()?;
    if !canonical.is_dir() {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, format!("library root is not a directory: {}", path.display())));
    }
    Ok(canonical)
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct LibraryEntry {
    #[specta(type = String)]
    library_id: LibraryId,
    library_name: String,
    #[specta(type = String)]
    library_path: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct StoredEntry {
    library_id: LibraryId,
    library_name: String,
    library_path: String,
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    library_id: LibraryId,
    library_name: String,
}

impl LibraryEntry {
    fn validated(library_id: LibraryId, name: &str, path: &Path) -> RegistryResult<Self> {
        let library_name = validate_name(name)?;
        let library_path = canonical_library_path(path)?;
        Ok(Self { library_id, library_name, library_path })
    }
    pub fn library_id(&self) -> &LibraryId {
        &self.library_id
    }
    pub fn library_name(&self) -> &str {
        &self.library_name
    }
    pub fn library_path(&self) -> &Path {
        &self.library_path
    }
}

#[derive(Clone, Debug)]
pub struct LibraryRegistry {
    app_data_root: PathBuf,
}

impl LibraryRegistry {
    pub fn open(app_data_root: impl Into<PathBuf>) -> RegistryResult<Self> {
        let registry = Self { app_data_root: app_data_root.into() };
        fs::create_dir_all(&registry.app_data_root)?;
        for entry in registry.entries()? {
            match read_library_manifest(entry.library_path())? {
                None => write_manifest(&entry)?,
                Some((id, _)) if id == *entry.library_id() => {}
                Some((id, _)) => return Err(format!("library manifest at {} identifies {}, but registry identifies {}", entry.library_path().display(), id, entry.library_id()).into()),
            }
        }
        Ok(registry)
    }

    fn csv_path(&self) -> PathBuf {
        self.app_data_root.join("libraries.csv")
    }

    fn detached_path(&self) -> PathBuf {
        self.app_data_root.join(DETACHED_LIBRARIES_FILENAME)
    }

    pub fn is_detached(&self, library_id: &LibraryId) -> RegistryResult<bool> {
        Ok(read_detached(&self.detached_path())?.contains(library_id))
    }

    pub fn pending_server_deletions(&self) -> RegistryResult<Vec<LibraryId>> {
        Ok(read_detached(&self.detached_path())?.into_iter().collect())
    }

    pub fn complete_server_deletion(&self, library_id: &LibraryId) -> RegistryResult<()> {
        let mut detached = read_detached(&self.detached_path())?;
        if detached.remove(library_id) {
            write_detached(&self.detached_path(), &detached)?;
        }
        Ok(())
    }

    pub fn entries(&self) -> RegistryResult<Vec<LibraryEntry>> {
        read_entries(&self.csv_path())
    }

    pub fn create(&self, name: &str, root: Option<PathBuf>) -> RegistryResult<LibraryEntry> {
        let generated = LibraryId::new_v4();
        let requested = root.unwrap_or_else(|| self.app_data_root.join("libraries").join(generated.to_string()));
        fs::create_dir_all(&requested)?;
        let root = canonical_library_path(&requested)?;
        let mut entries = self.entries()?;
        if entries.iter().any(|entry| entry.library_path() == root) {
            return Err(format!("library path already exists: {}", root.display()).into());
        }
        let (id, name) = read_library_manifest(&root)?.unwrap_or((generated, validate_name(if name.trim().is_empty() { "Library" } else { name })?));
        let entry = LibraryEntry::validated(id, &name, &root)?;
        write_manifest(&entry)?;
        if let Some(index) = entries.iter().position(|existing| existing.library_id() == &id) {
            entries[index] = entry.clone();
        } else {
            entries.push(entry.clone());
        }
        write_entries(&self.csv_path(), &entries)?;
        Ok(entry)
    }

    pub fn attach_remote(&self, id: &LibraryId, name: &str) -> RegistryResult<LibraryEntry> {
        let mut entries = self.entries()?;
        let path = entries.iter().find(|entry| entry.library_id() == id).map(|entry| entry.library_path().to_path_buf()).unwrap_or_else(|| self.app_data_root.join("libraries").join(id.to_string()));
        fs::create_dir_all(&path)?;
        let entry = LibraryEntry::validated(*id, name, &path)?;
        write_manifest(&entry)?;
        if let Some(index) = entries.iter().position(|existing| existing.library_id() == id) {
            entries[index] = entry.clone();
        } else {
            entries.push(entry.clone());
        }
        write_entries(&self.csv_path(), &entries)?;
        Ok(entry)
    }

    pub fn discover(&self, parent: &Path) -> RegistryResult<Vec<LibraryEntry>> {
        fs::create_dir_all(parent)?;
        let mut entries = self.entries()?;
        let mut paths = fs::read_dir(parent)?.filter_map(Result::ok).filter_map(|entry| entry.file_type().ok().filter(|kind| kind.is_dir()).map(|_| entry.path())).collect::<Vec<_>>();
        paths.sort();
        let mut discovered = Vec::new();
        for path in paths {
            let path = canonical_library_path(&path)?;
            let Some((id, name)) = read_library_manifest(&path)? else { continue };
            if let Some(existing) = entries.iter().find(|entry| entry.library_id() == &id) {
                discovered.push(existing.clone());
                continue;
            }
            if entries.iter().any(|entry| entry.library_path() == path) {
                return Err(format!("library path {} has conflicting identity", path.display()).into());
            }
            let entry = LibraryEntry::validated(id, &name, &path)?;
            entries.push(entry.clone());
            discovered.push(entry);
        }
        write_entries(&self.csv_path(), &entries)?;
        Ok(discovered)
    }

    pub fn remove(&self, path: &Path) -> RegistryResult<()> {
        let root = canonical_library_path(path)?;
        let original_entries = self.entries()?;
        let mut entries = original_entries.clone();
        let Some(index) = entries.iter().position(|entry| entry.library_path() == root) else {
            return Err("library path is not registered".into());
        };
        let removed_library_id = *entries.remove(index).library_id();
        let database_directory = root.join(METADATA_DIRECTORY);
        // Only Bokheim-owned catalogue state is removed. Book files and any
        // unrelated files below the library root must remain untouched.
        let state_paths = [root.join(LIBRARY_MANIFEST_FILENAME), database_directory.join("library.db"), database_directory.join("library.db-wal"), database_directory.join("library.db-shm"), database_directory.join("session.json")];
        let mut parked = Vec::new();
        let park_result = (|| -> RegistryResult<()> {
            for state_path in state_paths {
                match fs::symlink_metadata(&state_path) {
                    Ok(value) if value.file_type().is_symlink() => return Err(format!("refusing to remove symlinked library state file: {}", state_path.display()).into()),
                    Ok(value) if value.is_file() => {}
                    Ok(_) => return Err(format!("library state path is not a file: {}", state_path.display()).into()),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error.into()),
                }
                let file_name = state_path.file_name().ok_or("library state file has no name")?.to_string_lossy();
                let parent = state_path.parent().ok_or("library state file has no parent")?;
                let parked_path = parent.join(format!(".{file_name}-removing-{}", Uuid::new_v4()));
                fs::rename(&state_path, &parked_path)?;
                parked.push((state_path, parked_path));
            }
            Ok(())
        })();
        if let Err(error) = park_result {
            for (database, parked_path) in parked.into_iter().rev() {
                let _ = fs::rename(parked_path, database);
            }
            return Err(error);
        }
        if let Err(error) = write_entries(&self.csv_path(), &entries) {
            for (database, parked_path) in parked.into_iter().rev() {
                let _ = fs::rename(parked_path, database);
            }
            return Err(error);
        }
        if let Err(error) = remember_detached(&self.detached_path(), removed_library_id) {
            let _ = write_entries(&self.csv_path(), &original_entries);
            for (state_path, parked_path) in parked.into_iter().rev() {
                let _ = fs::rename(parked_path, state_path);
            }
            return Err(error);
        }
        for (_, parked_path) in parked {
            fs::remove_file(parked_path)?;
        }
        Ok(())
    }
}

fn read_detached(path: &Path) -> RegistryResult<HashSet<LibraryId>> {
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(HashSet::new()),
        Err(error) => Err(error.into()),
    }
}

fn remember_detached(path: &Path, library_id: LibraryId) -> RegistryResult<()> {
    let mut detached = read_detached(path)?;
    if !detached.insert(library_id) {
        return Ok(());
    }
    write_detached(path, &detached)
}

fn write_detached(path: &Path, detached: &HashSet<LibraryId>) -> RegistryResult<()> {
    let temporary = path.with_extension(format!("json-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        fs::write(&temporary, serde_json::to_vec(&detached)?)?;
        fs::rename(&temporary, path)?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub fn read_library_manifest(root: &Path) -> RegistryResult<Option<(LibraryId, String)>> {
    let path = root.join(LIBRARY_MANIFEST_FILENAME);
    let contents = match fs::read_to_string(&path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let manifest: Manifest = serde_json::from_str(&contents)?;
    if manifest.format != MANIFEST_FORMAT || manifest.version != MANIFEST_VERSION {
        return Err(format!("unsupported library manifest in {}", path.display()).into());
    }
    Ok(Some((manifest.library_id, validate_name(&manifest.library_name)?)))
}

fn validate_name(value: &str) -> RegistryResult<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 255 || value.chars().any(char::is_control) {
        return Err("invalid library name".into());
    }
    Ok(value.to_owned())
}

fn write_manifest(entry: &LibraryEntry) -> RegistryResult<()> {
    let path = entry.library_path().join(LIBRARY_MANIFEST_FILENAME);
    let temporary = entry.library_path().join(format!(".{LIBRARY_MANIFEST_FILENAME}-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        serde_json::to_writer_pretty(&mut file, &Manifest { format: MANIFEST_FORMAT.into(), version: MANIFEST_VERSION, library_id: *entry.library_id(), library_name: entry.library_name().into() })?;
        use std::io::Write;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn write_entries(path: &Path, entries: &[LibraryEntry]) -> RegistryResult<()> {
    let parent = path.parent().ok_or("registry has no parent")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".libraries-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        {
            let mut writer = WriterBuilder::new().has_headers(true).from_writer(&mut file);
            for entry in entries {
                writer.serialize(StoredEntry { library_id: *entry.library_id(), library_name: entry.library_name().into(), library_path: entry.library_path().to_string_lossy().into() })?;
            }
            writer.flush()?;
        }
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn read_entries(path: &Path) -> RegistryResult<Vec<LibraryEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    Reader::from_path(path)?
        .deserialize::<StoredEntry>()
        .map(|row| {
            let row = row?;
            LibraryEntry::validated(row.library_id, &row.library_name, Path::new(&row.library_path))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_canonicalizes_entries_and_round_trips() {
        let temporary = tempfile::tempdir().unwrap();
        let registry = LibraryRegistry::open(temporary.path().join("app")).unwrap();
        let root = temporary.path().join("library");
        let entry = registry.create(" Books ", Some(root.clone())).unwrap();

        assert_eq!(entry.library_name(), "Books");
        assert_eq!(entry.library_path(), root.canonicalize().unwrap());
        assert_eq!(registry.entries().unwrap().len(), 1);
        assert_eq!(read_library_manifest(&root).unwrap().unwrap().0, *entry.library_id());
    }

    #[test]
    fn remote_attachment_preserves_identity_and_updates_name() {
        let temporary = tempfile::tempdir().unwrap();
        let registry = LibraryRegistry::open(temporary.path().join("app")).unwrap();
        let id = LibraryId::new_v4();

        registry.attach_remote(&id, "Books").unwrap();
        let renamed = registry.attach_remote(&id, "Renamed").unwrap();

        assert_eq!(renamed.library_id(), &id);
        assert_eq!(renamed.library_name(), "Renamed");
        assert_eq!(registry.entries().unwrap().len(), 1);

        drop(registry);
        let reopened = LibraryRegistry::open(temporary.path().join("app")).unwrap();
        let persisted = reopened.entries().unwrap();
        assert_eq!(persisted.len(), 1);
        assert_eq!(persisted[0].library_id(), &id);
        assert_eq!(persisted[0].library_name(), "Renamed");
    }

    #[test]
    fn remove_deletes_bokheim_state_but_preserves_library_files() {
        let temporary = tempfile::tempdir().unwrap();
        let registry = LibraryRegistry::open(temporary.path().join("app")).unwrap();
        let root = temporary.path().join("library");
        let entry = registry.create("Books", Some(root.clone())).unwrap();
        let metadata = root.join(METADATA_DIRECTORY);
        fs::create_dir_all(metadata.join("thumbnails")).unwrap();
        fs::write(metadata.join("library.db"), b"sqlite").unwrap();
        fs::write(metadata.join("library.db-wal"), b"wal").unwrap();
        fs::write(metadata.join("library.db-shm"), b"shm").unwrap();
        fs::write(metadata.join("session.json"), b"session").unwrap();
        fs::write(metadata.join("keep-me"), b"metadata").unwrap();
        fs::write(root.join("book.epub"), b"book").unwrap();

        registry.remove(entry.library_path()).unwrap();

        assert!(registry.entries().unwrap().is_empty());
        assert!(registry.is_detached(entry.library_id()).unwrap());
        assert!(!root.join(LIBRARY_MANIFEST_FILENAME).exists());
        assert!(metadata.join("thumbnails").is_dir());
        assert!(metadata.join("keep-me").is_file());
        assert!(root.join("book.epub").is_file());
        assert!(!metadata.join("library.db").exists());
        assert!(!metadata.join("library.db-wal").exists());
        assert!(!metadata.join("library.db-shm").exists());
        assert!(!metadata.join("session.json").exists());

        drop(registry);
        let reopened = LibraryRegistry::open(temporary.path().join("app")).unwrap();
        assert!(reopened.is_detached(entry.library_id()).unwrap());
        assert_eq!(reopened.pending_server_deletions().unwrap(), vec![*entry.library_id()]);
        reopened.complete_server_deletion(entry.library_id()).unwrap();
        assert!(!reopened.is_detached(entry.library_id()).unwrap());
    }
}
