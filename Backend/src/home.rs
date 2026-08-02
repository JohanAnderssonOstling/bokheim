use crate::path_types::LibraryRoot;
use crate::{BookId, ServerUrl};
use csv::{Reader, WriterBuilder};
use serde::{Deserialize, Serialize};
use specta::Type;
use std::fs;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use sync_common::LibraryId;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Type)]
pub struct LibrarySyncBinding {
    server_url: String,
    account_id: String,
}

impl LibrarySyncBinding {
    pub(crate) fn from_session(session: &crate::auth::Session) -> Self {
        Self { server_url: session.server_url().as_str().to_owned(), account_id: session.user_id().to_owned() }
    }

    pub(crate) fn validated(server_url: &str, account_id: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let server_url = ServerUrl::parse(server_url)?;
        if server_url != crate::api_server_url() {
            return Err(format!("library sync must use {}", crate::API_SERVER_ORIGIN).into());
        }
        crate::auth::validate_user_id(account_id)?;
        Ok(Self { server_url: server_url.as_str().to_owned(), account_id: account_id.to_owned() })
    }

    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    pub fn account_id(&self) -> &str {
        &self.account_id
    }

    pub(crate) fn matches_session(&self, session: &crate::auth::Session) -> bool {
        self.server_url == session.server_url().as_str() && self.account_id == session.user_id()
    }
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct LibraryEntry {
    #[specta(type = String)]
    library_id: LibraryId,
    library_name: String,
    #[specta(type = String)]
    library_path: PathBuf,
    sync_binding: Option<LibrarySyncBinding>,
}

#[derive(Serialize, Deserialize)]
struct StoredLibraryEntry {
    library_id: String,
    library_name: String,
    library_path: String,
    #[serde(default)]
    sync_server_url: Option<String>,
    #[serde(default)]
    sync_account_id: Option<String>,
}

impl LibraryEntry {
    fn validated(library_id: &str, library_name: &str, library_path: &Path, sync_binding: Option<LibrarySyncBinding>) -> Result<Self, Box<dyn std::error::Error>> {
        let library_id = LibraryId::parse(library_id)?;
        let library_name = validate_library_name(library_name)?;
        let library_root = LibraryRoot::open(library_path)?;
        let library_path = library_root.as_path().to_str().ok_or("library path is not valid UTF-8")?;
        Ok(Self { library_id, library_name, library_path: PathBuf::from(library_path), sync_binding })
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

    pub fn sync_binding(&self) -> Option<&LibrarySyncBinding> {
        self.sync_binding.as_ref()
    }
}

impl StoredLibraryEntry {
    fn from_entry(entry: &LibraryEntry) -> Self {
        Self {
            library_id: entry.library_id.as_str().to_owned(),
            library_name: entry.library_name.clone(),
            library_path: entry.library_path.to_string_lossy().into_owned(),
            sync_server_url: entry.sync_binding.as_ref().map(|binding| binding.server_url.clone()),
            sync_account_id: entry.sync_binding.as_ref().map(|binding| binding.account_id.clone()),
        }
    }

    fn sync_binding(&self) -> Result<Option<LibrarySyncBinding>, Box<dyn std::error::Error>> {
        match (self.sync_server_url.as_deref(), self.sync_account_id.as_deref()) {
            (None, None) | (Some(""), None) | (None, Some("")) | (Some(""), Some("")) => Ok(None),
            (Some(server_url), Some(account_id)) if !server_url.is_empty() && !account_id.is_empty() => Ok(Some(LibrarySyncBinding::validated(server_url, account_id)?)),
            _ => Err("library sync binding must contain both server and account identity".into()),
        }
    }
}

fn validate_library_name(value: &str) -> Result<String, Box<dyn std::error::Error>> {
    let value = value.trim();
    if value.is_empty() || value.len() > 255 || value.chars().any(char::is_control) {
        return Err("library name must be non-empty, bounded, and contain no control characters".into());
    }
    Ok(value.to_owned())
}

/// Selects where device-local application state is stored.
///
/// Desktop frontends share `Bokheim`'s conventional data directory. Sandboxed
/// platforms such as Android provide the private directory assigned by the OS.
#[derive(Clone, Debug)]
pub enum AppDataLocation {
    SharedDesktop,
    PlatformLocal(PathBuf),
}

#[derive(Clone, Debug)]
struct AppDataRoot(PathBuf);

impl AppDataRoot {
    fn initialize(location: AppDataLocation) -> Result<Self, String> {
        let path = match location {
            AppDataLocation::SharedDesktop => dirs::data_dir().ok_or("failed to resolve desktop data dir")?.join(crate::APP_NAME_LOWER),
            AppDataLocation::PlatformLocal(path) => path,
        };
        fs::create_dir_all(&path).map_err(|error| format!("failed to create app data directory {}: {error}", path.display()))?;
        let path = path.canonicalize().map_err(|error| format!("failed to validate app data directory {}: {error}", path.display()))?;
        if !path.is_dir() {
            return Err(format!("app data path is not a directory: {}", path.display()));
        }
        Ok(Self(path))
    }

    fn as_path(&self) -> &Path {
        &self.0
    }
}

/// Validated device-local backend state. Any operation needing application
/// data requires this value, so an uninitialized backend cannot be expressed.
#[derive(Clone, Debug)]
pub struct BackendContext {
    app_data_root: AppDataRoot,
    transfer_runtime: crate::transfer::TransferRuntime,
}

impl BackendContext {
    pub fn initialize(location: AppDataLocation) -> Result<Self, String> {
        Ok(Self { app_data_root: AppDataRoot::initialize(location)?, transfer_runtime: crate::transfer::TransferRuntime::default() })
    }

    pub fn app_data_dir(&self) -> &Path {
        self.app_data_root.as_path()
    }

    pub(crate) fn transfer_runtime(&self) -> crate::transfer::TransferRuntime {
        self.transfer_runtime.clone()
    }

    pub fn try_get_thumbnail(&self, book_id: BookId) -> std::io::Result<Option<Vec<u8>>> {
        match self.resolve_thumbnail(book_id)? {
            None => Ok(None),
            Some(thumb_path) => fs::read(thumb_path).map(Some),
        }
    }

    pub fn resolve_thumbnail(&self, book_id: BookId) -> std::io::Result<Option<PathBuf>> {
        let thumb_path = self.app_data_root.as_path().join(crate::APP_HIDDEN_THUMBNAILS_PATH).join(format!("{book_id}.jpg"));
        match fs::metadata(&thumb_path) {
            Ok(metadata) if metadata.is_file() => Ok(Some(thumb_path)),
            Ok(_) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("thumbnail path is not a file: {}", thumb_path.display()))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn thumbnails_dir(&self) -> std::io::Result<PathBuf> {
        let thumbnails_dir = self.app_data_root.as_path().join(crate::APP_HIDDEN_THUMBNAILS_PATH);
        fs::create_dir_all(&thumbnails_dir)?;
        Ok(thumbnails_dir)
    }

    pub fn create_library(&self, name: String, root: Option<PathBuf>) -> Result<LibraryEntry, Box<dyn std::error::Error>> {
        let csv_path = self.app_data_root.as_path().join("libraries.csv");
        let library_id = LibraryId::parse(&Uuid::new_v4().to_string())?;
        let requested_root = root.unwrap_or_else(|| self.app_data_root.as_path().join("libraries").join(library_id.as_str()));
        ensure_dir(&requested_root)?;
        let library_root = LibraryRoot::open(&requested_root)?;
        let mut entries = self.try_get_library_entries()?;
        if entries.iter().any(|entry| entry.library_path() == library_root.as_path()) {
            return Err(format!("library path already exists: {}", library_root.as_path().display()).into());
        }
        let name = if name.trim().is_empty() { "Library" } else { &name };
        let entry = LibraryEntry::validated(library_id.as_str(), name, library_root.as_path(), None)?;
        entries.push(entry.clone());
        write_library_entries_atomic(&csv_path, &entries)?;
        Ok(entry)
    }

    /// Registers a server-owned library locally without changing its stable ID.
    /// Existing entries are renamed from the authoritative server catalogue.
    pub fn attach_remote_library(&self, library_id: &LibraryId, name: &str, binding: LibrarySyncBinding) -> Result<LibraryEntry, Box<dyn std::error::Error>> {
        let csv_path = self.app_data_root.as_path().join("libraries.csv");
        let mut entries = self.try_get_library_entries()?;
        if let Some(index) = entries.iter().position(|entry| entry.library_id() == library_id) {
            let path = entries[index].library_path().to_path_buf();
            let updated = LibraryEntry::validated(library_id.as_str(), name, &path, Some(binding))?;
            entries[index] = updated.clone();
            write_library_entries_atomic(&csv_path, &entries)?;
            return Ok(updated);
        }

        let root = self.app_data_root.as_path().join("libraries").join(library_id.as_str());
        ensure_dir(&root)?;
        let entry = LibraryEntry::validated(library_id.as_str(), name, &root, Some(binding))?;
        entries.push(entry.clone());
        write_library_entries_atomic(&csv_path, &entries)?;
        Ok(entry)
    }

    pub fn get_libraries(&self) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        Ok(self.try_get_library_entries()?.into_iter().map(|entry| entry.library_path).collect())
    }

    pub fn try_get_library_entries(&self) -> Result<Vec<LibraryEntry>, Box<dyn std::error::Error>> {
        read_library_entries(&self.app_data_root.as_path().join("libraries.csv"))
    }

    pub fn get_library_path_by_id(&self, library_id: &LibraryId) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
        Ok(self.try_get_library_entries()?.into_iter().find(|entry| &entry.library_id == library_id).map(|entry| entry.library_path))
    }

    pub fn set_library_sync_binding(&self, library_id: &LibraryId, binding: LibrarySyncBinding) -> Result<LibraryEntry, Box<dyn std::error::Error>> {
        self.update_library_sync_binding(library_id, Some(binding))
    }

    pub fn clear_library_sync_binding(&self, library_id: &LibraryId) -> Result<LibraryEntry, Box<dyn std::error::Error>> {
        self.update_library_sync_binding(library_id, None)
    }

    fn update_library_sync_binding(&self, library_id: &LibraryId, sync_binding: Option<LibrarySyncBinding>) -> Result<LibraryEntry, Box<dyn std::error::Error>> {
        let csv_path = self.app_data_root.as_path().join("libraries.csv");
        let mut entries = self.try_get_library_entries()?;
        let index = entries.iter().position(|entry| entry.library_id() == library_id).ok_or_else(|| format!("library not found: {library_id}"))?;
        let current = &entries[index];
        let updated = LibraryEntry::validated(current.library_id.as_str(), &current.library_name, &current.library_path, sync_binding)?;
        entries[index] = updated.clone();
        write_library_entries_atomic(&csv_path, &entries)?;
        Ok(updated)
    }

    pub fn remove_library(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let requested_root = LibraryRoot::open(Path::new(path))?;
        let csv_path = self.app_data_root.as_path().join("libraries.csv");
        let entries = read_library_entries(&csv_path)?;
        let Some(index) = entries.iter().position(|entry| entry.library_path() == requested_root.as_path()) else {
            return Err(format!("library path is not registered: {}", requested_root.as_path().display()).into());
        };
        let mut remaining = entries;
        remaining.remove(index);
        let assets_dir = requested_root.as_path().join(crate::APP_HIDDEN_DIR);
        let parked_assets = requested_root.as_path().join(format!("{}-removing-{}", crate::APP_HIDDEN_DIR, Uuid::new_v4()));
        let assets_were_parked = match fs::symlink_metadata(&assets_dir) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(format!("refusing to remove symlinked library metadata: {}", assets_dir.display()).into()),
            Ok(metadata) if metadata.is_dir() => {
                fs::rename(&assets_dir, &parked_assets)?;
                true
            }
            Ok(_) => return Err(format!("library metadata is not a directory: {}", assets_dir.display()).into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if let Err(error) = write_library_entries_atomic(&csv_path, &remaining) {
            if assets_were_parked {
                if let Err(restore_error) = fs::rename(&parked_assets, &assets_dir) {
                    return Err(format!("registry update failed ({error}); metadata rollback also failed ({restore_error})").into());
                }
            }
            return Err(error);
        }
        if assets_were_parked {
            fs::remove_dir_all(&parked_assets)?;
        }
        Ok(())
    }
}

fn ensure_dir(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(path)?;
    Ok(())
}

fn write_library_entries_atomic(csv_path: &Path, entries: &[LibraryEntry]) -> Result<(), Box<dyn std::error::Error>> {
    let parent = csv_path.parent().ok_or("library registry has no parent directory")?;
    ensure_dir(parent)?;
    let temp_path = parent.join(format!(".libraries-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temp_path)?;
        {
            let mut writer = WriterBuilder::new().has_headers(true).from_writer(&mut file);
            for entry in entries {
                writer.serialize(StoredLibraryEntry::from_entry(entry))?;
            }
            writer.flush()?;
        }
        file.sync_all()?;
        fs::rename(&temp_path, csv_path)?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

fn read_library_entries(csv_path: &Path) -> Result<Vec<LibraryEntry>, Box<dyn std::error::Error>> {
    if !csv_path.exists() {
        return Ok(Vec::new());
    }
    let mut reader = Reader::from_path(csv_path)?;
    let mut entries = Vec::new();
    for row in reader.deserialize::<StoredLibraryEntry>() {
        let row = row?;
        let sync_binding = row.sync_binding()?;
        entries.push(LibraryEntry::validated(&row.library_id, &row.library_name, Path::new(&row.library_path), sync_binding)?);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_rows_are_canonicalized_and_validated() {
        let root = tempfile::tempdir().unwrap();
        let entry = LibraryEntry::validated("library-1", " Books ", root.path(), None).unwrap();
        assert_eq!(entry.library_name, "Books");
        assert_eq!(Path::new(&entry.library_path), root.path().canonicalize().unwrap());
        assert!(LibraryEntry::validated("bad/id", "Books", root.path(), None).is_err());
        assert!(LibraryEntry::validated("library-1", "\n", root.path(), None).is_err());
    }

    #[test]
    fn malformed_registry_rows_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let csv_path = temp.path().join("libraries.csv");
        fs::write(&csv_path, "library_id,library_name,library_path\nbad/id,Books,/missing\n").unwrap();
        assert!(read_library_entries(&csv_path).is_err());
    }

    #[test]
    fn atomic_registry_replacement_round_trips_valid_entries() {
        let temp = tempfile::tempdir().unwrap();
        let library = temp.path().join("library");
        fs::create_dir(&library).unwrap();
        let csv_path = temp.path().join("libraries.csv");
        let entry = LibraryEntry::validated("library-1", "Books", &library, None).unwrap();
        write_library_entries_atomic(&csv_path, std::slice::from_ref(&entry)).unwrap();
        let entries = read_library_entries(&csv_path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].library_id, entry.library_id);
    }

    #[test]
    fn backend_context_is_validated_and_instance_scoped() {
        let temp = tempfile::tempdir().unwrap();
        let first = BackendContext::initialize(AppDataLocation::PlatformLocal(temp.path().join("first"))).unwrap();
        let second = BackendContext::initialize(AppDataLocation::PlatformLocal(temp.path().join("second"))).unwrap();
        assert_ne!(first.app_data_dir(), second.app_data_dir());
        assert!(first.app_data_dir().is_dir());
        assert!(second.app_data_dir().is_dir());
    }

    #[test]
    fn remote_library_attachment_preserves_identity_and_updates_name() {
        let temp = tempfile::tempdir().unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(temp.path().join("app"))).unwrap();
        let id = LibraryId::parse("server-library").unwrap();
        let binding = LibrarySyncBinding::validated(crate::API_SERVER_ORIGIN, "user-1").unwrap();
        let first = context.attach_remote_library(&id, "Books", binding.clone()).unwrap();
        let second = context.attach_remote_library(&id, "Renamed", binding.clone()).unwrap();
        assert_eq!(first.library_id(), second.library_id());
        assert_eq!(second.library_name(), "Renamed");
        assert_eq!(second.sync_binding(), Some(&binding));
        assert_eq!(context.try_get_library_entries().unwrap().len(), 1);
    }

    #[test]
    fn registry_round_trips_and_clears_sync_binding() {
        let temp = tempfile::tempdir().unwrap();
        let context = BackendContext::initialize(AppDataLocation::PlatformLocal(temp.path().join("app"))).unwrap();
        let entry = context.create_library("Books".to_owned(), None).unwrap();
        assert!(entry.sync_binding().is_none());

        let binding = LibrarySyncBinding::validated(crate::API_SERVER_ORIGIN, "user-1").unwrap();
        let linked = context.set_library_sync_binding(entry.library_id(), binding.clone()).unwrap();
        assert_eq!(linked.sync_binding(), Some(&binding));
        assert_eq!(context.try_get_library_entries().unwrap()[0].sync_binding(), Some(&binding));

        let local = context.clear_library_sync_binding(entry.library_id()).unwrap();
        assert!(local.sync_binding().is_none());
        assert!(context.try_get_library_entries().unwrap()[0].sync_binding().is_none());
    }

    #[test]
    fn sync_bindings_reject_non_api_servers() {
        assert!(LibrarySyncBinding::validated("https://example.com", "user-1").is_err());
    }
}
