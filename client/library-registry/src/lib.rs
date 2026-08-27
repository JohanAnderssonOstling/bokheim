//! SQLite-backed device catalogue of library storage roots.
//!
//! Registry and key/value behavior is identical on every target. Native code
//! supplies real directory roots; the browser supplies SQLite VFS filenames.

use rusqlite::{params, Connection, OptionalExtension};
#[cfg(not(target_arch = "wasm32"))]
use serde::Deserialize;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use sync_common::LibraryId;

#[cfg(not(target_arch = "wasm32"))]
pub const LIBRARY_MANIFEST_FILENAME: &str = ".bokheim-library.json";
#[cfg(not(target_arch = "wasm32"))]
const MANIFEST_FORMAT: &str = "bokheim-library";
#[cfg(not(target_arch = "wasm32"))]
const MANIFEST_VERSION: u32 = 1;
#[cfg(not(target_arch = "wasm32"))]
const METADATA_DIRECTORY: &str = ".bokheim";
const DATABASE_FILENAME: &str = "app-state-v1.sqlite3";

const SCHEMA: &str = r#"
    CREATE TABLE IF NOT EXISTS library_registry (
        library_id TEXT PRIMARY KEY NOT NULL,
        library_name TEXT NOT NULL CHECK(length(trim(library_name)) BETWEEN 1 AND 255),
        storage_path TEXT NOT NULL UNIQUE
    ) STRICT;
    CREATE TABLE IF NOT EXISTS detached_library (
        library_id TEXT PRIMARY KEY NOT NULL
    ) STRICT;
    CREATE TABLE IF NOT EXISTS app_value (
        key TEXT PRIMARY KEY NOT NULL,
        value BLOB NOT NULL
    ) STRICT;
"#;

pub type RegistryResult<T> = Result<T, Box<dyn std::error::Error>>;

#[derive(Clone, Debug, Serialize)]
pub struct LibraryEntry {
    library_id: LibraryId,
    library_name: String,
    library_path: PathBuf,
}

impl LibraryEntry {
    fn new(library_id: LibraryId, name: &str, library_path: PathBuf) -> RegistryResult<Self> {
        Ok(Self { library_id, library_name: validate_name(name)?, library_path })
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

#[derive(Clone)]
pub struct LibraryRegistry {
    app_data_root: PathBuf,
    connection: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for LibraryRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("LibraryRegistry").field("app_data_root", &self.app_data_root).finish_non_exhaustive()
    }
}

impl LibraryRegistry {
    pub fn open(app_data_root: impl Into<PathBuf>) -> RegistryResult<Self> {
        let app_data_root = app_data_root.into();
        #[cfg(not(target_arch = "wasm32"))]
        std::fs::create_dir_all(&app_data_root)?;
        let database_path = app_data_root.join(DATABASE_FILENAME);
        let connection = Connection::open(database_path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(SCHEMA)?;
        Ok(Self { app_data_root, connection: Arc::new(Mutex::new(connection)) })
    }

    fn connection(&self) -> RegistryResult<MutexGuard<'_, Connection>> {
        self.connection.lock().map_err(|_| "library registry lock is poisoned".into())
    }

    pub fn entries(&self) -> RegistryResult<Vec<LibraryEntry>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT library_id,library_name,storage_path FROM library_registry ORDER BY library_name COLLATE NOCASE,library_id")?;
        let entries = statement
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?
            .map(|row| {
                let (id, name, path) = row?;
                LibraryEntry::new(LibraryId::parse_str(&id)?, &name, PathBuf::from(path))
            })
            .collect();
        entries
    }

    pub fn create(&self, name: &str, root: Option<PathBuf>) -> RegistryResult<LibraryEntry> {
        let generated_id = LibraryId::new_v4();
        let requested_name = if name.trim().is_empty() { "Library" } else { name };
        let (library_id, library_name, storage_path) = platform::prepare_created_library(&self.app_data_root, generated_id, requested_name, root)?;
        let entry = LibraryEntry::new(library_id, &library_name, storage_path)?;
        self.connection()?.execute(
            "INSERT INTO library_registry(library_id,library_name,storage_path) VALUES(?1,?2,?3) ON CONFLICT(library_id) DO UPDATE SET library_name=excluded.library_name,storage_path=excluded.storage_path",
            params![entry.library_id.to_string(), entry.library_name, entry.library_path.to_string_lossy()],
        )?;
        Ok(entry)
    }

    pub fn attach_remote(&self, id: &LibraryId, name: &str) -> RegistryResult<LibraryEntry> {
        let existing_path = self.entries()?.into_iter().find(|entry| entry.library_id() == id).map(|entry| entry.library_path);
        let storage_path = platform::prepare_attached_library(&self.app_data_root, *id, name, existing_path)?;
        let entry = LibraryEntry::new(*id, name, storage_path)?;
        self.connection()?.execute(
            "INSERT INTO library_registry(library_id,library_name,storage_path) VALUES(?1,?2,?3) ON CONFLICT(library_id) DO UPDATE SET library_name=excluded.library_name",
            params![id.to_string(), entry.library_name, entry.library_path.to_string_lossy()],
        )?;
        Ok(entry)
    }

    pub fn is_detached(&self, library_id: &LibraryId) -> RegistryResult<bool> {
        Ok(self.connection()?.query_row("SELECT EXISTS(SELECT 1 FROM detached_library WHERE library_id=?1)", [library_id.to_string()], |row| row.get(0))?)
    }

    pub fn pending_server_deletions(&self) -> RegistryResult<Vec<LibraryId>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT library_id FROM detached_library ORDER BY library_id")?;
        let ids = statement.query_map([], |row| row.get::<_, String>(0))?.map(|row| Ok(LibraryId::parse_str(&row?)?)).collect();
        ids
    }

    pub fn complete_server_deletion(&self, library_id: &LibraryId) -> RegistryResult<()> {
        self.connection()?.execute("DELETE FROM detached_library WHERE library_id=?1", [library_id.to_string()])?;
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn discover(&self, parent: &Path) -> RegistryResult<Vec<LibraryEntry>> {
        let discovered = platform::discover_libraries(parent)?;
        let mut registered = Vec::new();
        for (id, name, path) in discovered {
            if let Some(existing) = self.entries()?.into_iter().find(|entry| entry.library_id() == &id) {
                registered.push(existing);
                continue;
            }
            let entry = LibraryEntry::new(id, &name, path)?;
            self.connection()?.execute("INSERT INTO library_registry(library_id,library_name,storage_path) VALUES(?1,?2,?3)", params![entry.library_id.to_string(), entry.library_name, entry.library_path.to_string_lossy()])?;
            registered.push(entry);
        }
        Ok(registered)
    }

    pub fn remove(&self, path: &Path) -> RegistryResult<()> {
        let entry = self.entries()?.into_iter().find(|entry| entry.library_path() == path).ok_or("library path is not registered")?;
        platform::remove_library_state(&entry)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute("INSERT OR IGNORE INTO detached_library(library_id) VALUES(?1)", [entry.library_id.to_string()])?;
        transaction.execute("DELETE FROM library_registry WHERE library_id=?1", [entry.library_id.to_string()])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn read_value(&self, key: &str) -> RegistryResult<Option<Vec<u8>>> {
        Ok(self.connection()?.query_row("SELECT value FROM app_value WHERE key=?1", [key], |row| row.get(0)).optional()?)
    }

    pub fn write_value(&self, key: &str, value: &[u8]) -> RegistryResult<()> {
        self.connection()?.execute("INSERT INTO app_value(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![key, value])?;
        Ok(())
    }

    pub fn remove_value(&self, key: &str) -> RegistryResult<()> {
        self.connection()?.execute("DELETE FROM app_value WHERE key=?1", [key])?;
        Ok(())
    }
}

fn validate_name(value: &str) -> RegistryResult<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 255 || value.chars().any(char::is_control) {
        return Err("invalid library name".into());
    }
    Ok(value.to_owned())
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Serialize, Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    library_id: LibraryId,
    library_name: String,
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_library_manifest(root: &Path) -> RegistryResult<Option<(LibraryId, String)>> {
    platform::read_manifest(root)
}

#[cfg(not(target_arch = "wasm32"))]
mod platform {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use uuid::Uuid;

    fn canonical_library_path(path: &Path) -> RegistryResult<PathBuf> {
        let canonical = path.canonicalize()?;
        if !canonical.is_dir() {
            return Err(format!("library root is not a directory: {}", path.display()).into());
        }
        Ok(canonical)
    }

    pub(super) fn prepare_created_library(root: &Path, generated_id: LibraryId, name: &str, requested: Option<PathBuf>) -> RegistryResult<(LibraryId, String, PathBuf)> {
        let requested = requested.unwrap_or_else(|| root.join("libraries").join(generated_id.to_string()));
        fs::create_dir_all(&requested)?;
        let requested = canonical_library_path(&requested)?;
        let (id, name) = read_manifest(&requested)?.unwrap_or((generated_id, validate_name(name)?));
        let entry = LibraryEntry::new(id, &name, requested.clone())?;
        write_manifest(&entry)?;
        Ok((id, name, requested))
    }

    pub(super) fn prepare_attached_library(root: &Path, id: LibraryId, name: &str, existing: Option<PathBuf>) -> RegistryResult<PathBuf> {
        let path = existing.unwrap_or_else(|| root.join("libraries").join(id.to_string()));
        fs::create_dir_all(&path)?;
        let path = canonical_library_path(&path)?;
        write_manifest(&LibraryEntry::new(id, name, path.clone())?)?;
        Ok(path)
    }

    pub(super) fn discover_libraries(parent: &Path) -> RegistryResult<Vec<(LibraryId, String, PathBuf)>> {
        fs::create_dir_all(parent)?;
        let mut paths = fs::read_dir(parent)?.filter_map(Result::ok).filter_map(|entry| entry.file_type().ok().filter(|kind| kind.is_dir()).map(|_| entry.path())).collect::<Vec<_>>();
        paths.sort();
        let mut found = Vec::new();
        for path in paths {
            let path = canonical_library_path(&path)?;
            if let Some((id, name)) = read_manifest(&path)? {
                found.push((id, name, path));
            }
        }
        Ok(found)
    }

    pub(super) fn remove_library_state(entry: &LibraryEntry) -> RegistryResult<()> {
        let root = entry.library_path();
        let metadata = root.join(METADATA_DIRECTORY);
        for path in [root.join(LIBRARY_MANIFEST_FILENAME), metadata.join("library.db"), metadata.join("library.db-wal"), metadata.join("library.db-shm"), metadata.join("session.json")] {
            match fs::symlink_metadata(&path) {
                Ok(value) if value.file_type().is_symlink() => return Err(format!("refusing to remove symlinked library state file: {}", path.display()).into()),
                Ok(value) if value.is_file() => fs::remove_file(path)?,
                Ok(_) => return Err(format!("library state path is not a file: {}", path.display()).into()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    pub(super) fn read_manifest(root: &Path) -> RegistryResult<Option<(LibraryId, String)>> {
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

    fn write_manifest(entry: &LibraryEntry) -> RegistryResult<()> {
        let path = entry.library_path().join(LIBRARY_MANIFEST_FILENAME);
        let temporary = entry.library_path().join(format!(".{LIBRARY_MANIFEST_FILENAME}-{}.tmp", Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new().write(true).create_new(true).open(&temporary)?;
            serde_json::to_writer_pretty(&mut file, &Manifest { format: MANIFEST_FORMAT.into(), version: MANIFEST_VERSION, library_id: *entry.library_id(), library_name: entry.library_name().into() })?;
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
}

#[cfg(target_arch = "wasm32")]
mod platform {
    use super::*;
    use std::ffi::CString;

    pub(super) fn prepare_created_library(_root: &Path, id: LibraryId, name: &str, requested: Option<PathBuf>) -> RegistryResult<(LibraryId, String, PathBuf)> {
        if requested.is_some() {
            return Err("browser libraries do not accept native filesystem roots".into());
        }
        Ok((id, validate_name(name)?, PathBuf::from(format!("bokheim-library-v1-{id}.sqlite3"))))
    }

    pub(super) fn prepare_attached_library(_root: &Path, id: LibraryId, _name: &str, existing: Option<PathBuf>) -> RegistryResult<PathBuf> {
        Ok(existing.unwrap_or_else(|| PathBuf::from(format!("bokheim-library-v1-{id}.sqlite3"))))
    }

    pub(super) fn remove_library_state(entry: &LibraryEntry) -> RegistryResult<()> {
        let filename = CString::new(entry.library_path().to_string_lossy().as_bytes())?;
        // The registered SQLite VFS owns OPFS naming and deletion. Calling its
        // xDelete is the browser equivalent of removing the native database
        // file, and keeps that difference entirely at the storage boundary.
        let result = unsafe {
            let vfs = rusqlite::ffi::sqlite3_vfs_find(std::ptr::null());
            if vfs.is_null() {
                return Err("SQLite has no default VFS".into());
            }
            let delete = (*vfs).xDelete.ok_or("SQLite VFS does not support database deletion")?;
            delete(vfs, filename.as_ptr(), 1)
        };
        if result == rusqlite::ffi::SQLITE_OK || result == rusqlite::ffi::SQLITE_IOERR_DELETE_NOENT {
            Ok(())
        } else {
            Err(format!("SQLite VFS could not delete {}: error {result}", entry.library_path().display()).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_round_trips_through_sqlite() {
        let temporary = tempfile::tempdir().unwrap();
        let registry = LibraryRegistry::open(temporary.path().join("app")).unwrap();
        let root = temporary.path().join("library");
        let entry = registry.create(" Books ", Some(root)).unwrap();
        assert_eq!(entry.library_name(), "Books");
        assert_eq!(registry.entries().unwrap().len(), 1);
        drop(registry);
        assert_eq!(LibraryRegistry::open(temporary.path().join("app")).unwrap().entries().unwrap().len(), 1);
    }

    #[test]
    fn values_and_detached_libraries_share_the_store() {
        let temporary = tempfile::tempdir().unwrap();
        let registry = LibraryRegistry::open(temporary.path().join("app")).unwrap();
        registry.write_value("preference", b"value").unwrap();
        assert_eq!(registry.read_value("preference").unwrap().as_deref(), Some(b"value".as_slice()));
        let entry = registry.create("Books", None).unwrap();
        registry.remove(entry.library_path()).unwrap();
        assert!(registry.is_detached(entry.library_id()).unwrap());
        assert!(registry.entries().unwrap().is_empty());
    }
}
