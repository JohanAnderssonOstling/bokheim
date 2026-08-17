use crate::ContentHash;
pub(crate) use library_registry::read_library_manifest;
use library_registry::LibraryRegistry;
pub use library_registry::{LibraryEntry, LIBRARY_MANIFEST_FILENAME};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use sync_common::LibraryId;

const UNIFIED_TAXONOMY_RELATIVE_PATH: &str = "taxonomy/unified-taxonomy-v2.sqlite3";

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
    registry: LibraryRegistry,
    transfer_runtime: crate::sync::transfer::TransferRuntime,
    taxonomy_update: Arc<Mutex<()>>,
}

impl BackendContext {
    pub fn initialize(location: AppDataLocation) -> Result<Self, String> {
        let app_data_root = AppDataRoot::initialize(location)?;
        install_runtime_taxonomy(app_data_root.as_path())?;
        let registry = LibraryRegistry::open(app_data_root.as_path()).map_err(|error| format!("failed to initialize library registry: {error}"))?;
        Ok(Self { app_data_root, registry, transfer_runtime: crate::sync::transfer::TransferRuntime::default(), taxonomy_update: Arc::new(Mutex::new(())) })
    }

    pub fn app_data_dir(&self) -> &Path {
        self.app_data_root.as_path()
    }

    pub fn unified_taxonomy_path(&self) -> PathBuf {
        self.app_data_root.as_path().join(UNIFIED_TAXONOMY_RELATIVE_PATH)
    }

    pub fn reload_unified_taxonomy_from_disk(&self) -> Result<String, String> {
        let installed_path = self.unified_taxonomy_path();
        let development_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../library-model/data/unified-taxonomy-v2.sqlite3");
        let path = if development_path.is_file() { development_path } else { installed_path };
        let concepts = library_model::read_unified_taxonomy_sqlite(&path)?;
        library_model::install_unified_taxonomy_concepts(concepts).map_err(|error| format!("failed to install unified taxonomy database {}: {error}", path.display()))?;
        Ok(library_model::unified_taxonomy_release_id())
    }

    /// Persists an ancestor-closed server slice and atomically replaces the
    /// process-wide matcher with the resulting device-local taxonomy.
    pub(crate) fn unresolved_taxonomy_codes(&self, codes: Vec<taxonomy_contract::TaxonomyCode>) -> Result<Vec<taxonomy_contract::TaxonomyCode>, String> {
        let codes = codes
            .into_iter()
            .filter_map(|mut code| {
                code.code = code.code.trim().to_ascii_uppercase();
                (!code.code.is_empty()).then_some(code)
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let candidates = codes.iter().map(|code| (code.system.as_str().to_owned(), code.code.clone())).collect::<Vec<_>>();
        let resolved = library_model::read_resolved_unified_taxonomy_codes(&self.unified_taxonomy_path(), &candidates)?;
        Ok(codes.into_iter().filter(|code| !library_model::unified_has_exact_source_code(code.system.as_str(), &code.code) && !resolved.contains(&(code.system.as_str().to_owned(), code.code.clone()))).collect())
    }

    pub(crate) fn merge_unified_taxonomy_slice(&self, server_release_id: &str, concepts: &[library_model::UnifiedConceptDefinition], resolutions: &[library_model::UnifiedCodeResolution]) -> Result<String, String> {
        let _guard = self.taxonomy_update.lock().map_err(|_| "taxonomy update lock is poisoned".to_owned())?;
        let path = self.unified_taxonomy_path();
        library_model::merge_unified_taxonomy_sqlite_with_resolutions(&path, server_release_id, concepts, resolutions)?;
        let merged = library_model::read_unified_taxonomy_sqlite(&path)?;
        library_model::install_unified_taxonomy_concepts(merged).map_err(|error| format!("failed to install merged unified taxonomy database {}: {error}", path.display()))?;
        Ok(library_model::unified_taxonomy_release_id())
    }

    pub(crate) fn transfer_runtime(&self) -> crate::sync::transfer::TransferRuntime {
        self.transfer_runtime.clone()
    }

    pub fn try_get_thumbnail(&self, content_hash: ContentHash) -> std::io::Result<Option<Vec<u8>>> {
        match self.resolve_thumbnail(content_hash)? {
            None => Ok(None),
            Some(thumb_path) => fs::read(thumb_path).map(Some),
        }
    }

    pub fn resolve_thumbnail(&self, content_hash: ContentHash) -> std::io::Result<Option<PathBuf>> {
        let thumb_path = self.app_data_root.as_path().join(crate::APP_HIDDEN_THUMBNAILS_PATH).join(format!("{content_hash}.jpg"));
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
        self.registry.create(&name, root)
    }

    pub fn attach_remote_library(&self, library_id: &LibraryId, name: &str) -> Result<LibraryEntry, Box<dyn std::error::Error>> {
        self.registry.attach_remote(library_id, name)
    }

    pub fn is_library_detached(&self, library_id: &LibraryId) -> Result<bool, Box<dyn std::error::Error>> {
        self.registry.is_detached(library_id)
    }

    pub fn pending_library_deletions(&self) -> Result<Vec<LibraryId>, Box<dyn std::error::Error>> {
        self.registry.pending_server_deletions()
    }

    pub fn complete_library_deletion(&self, library_id: &LibraryId) -> Result<(), Box<dyn std::error::Error>> {
        self.registry.complete_server_deletion(library_id)
    }

    pub fn get_libraries(&self) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        Ok(self.registry.entries()?.into_iter().map(|entry| entry.library_path().to_path_buf()).collect())
    }

    pub fn try_get_library_entries(&self) -> Result<Vec<LibraryEntry>, Box<dyn std::error::Error>> {
        self.registry.entries()
    }

    pub fn discover_libraries(&self, libraries_root: &Path) -> Result<Vec<LibraryEntry>, Box<dyn std::error::Error>> {
        self.registry.discover(libraries_root)
    }

    pub fn get_library_path_by_id(&self, library_id: &LibraryId) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
        Ok(self.registry.entries()?.into_iter().find(|entry| entry.library_id() == library_id).map(|entry| entry.library_path().to_path_buf()))
    }

    pub fn remove_library(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.registry.remove(Path::new(path))
    }
}

fn install_runtime_taxonomy(app_data_root: &Path) -> Result<(), String> {
    let path = app_data_root.join(UNIFIED_TAXONOMY_RELATIVE_PATH);
    if !path.exists() {
        let parent = path.parent().ok_or_else(|| format!("taxonomy path has no parent: {}", path.display()))?;
        fs::create_dir_all(parent).map_err(|error| format!("failed to create taxonomy directory {}: {error}", parent.display()))?;
        fs::write(&path, library_model::DEFAULT_UNIFIED_TAXONOMY_SQLITE).map_err(|error| format!("failed to create editable taxonomy database {}: {error}", path.display()))?;
    }
    let concepts = library_model::read_unified_taxonomy_sqlite(&path)?;
    library_model::install_unified_taxonomy_concepts(concepts).map_err(|error| format!("failed to install unified taxonomy database {}: {error}", path.display()))
}
