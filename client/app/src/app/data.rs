use app_preferences::PreferenceStore;
pub use library_registry::LibraryEntry;
pub(crate) use library_registry::LibraryRegistry;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Selects where device-local application state is stored.
///
/// Desktop frontends share `Bokheim`'s conventional data directory. Sandboxed
/// platforms such as Android provide the private directory assigned by the OS.
#[derive(Clone, Debug)]
pub enum AppDataLocation {
    SharedDesktop,
    PlatformLocal(String),
}

impl AppDataLocation {
    pub(crate) fn data_dir(&self) -> Result<PathBuf, String> {
        match self {
            Self::SharedDesktop => client_platform_desktop::data_dir(),
            Self::PlatformLocal(locator) => Ok(locator.into()),
        }
    }

    pub(crate) fn default_library_root(&self) -> Result<std::path::PathBuf, String> {
        match self {
            Self::SharedDesktop => client_platform_desktop::default_library_root(),
            Self::PlatformLocal(locator) => Ok(Path::new(locator).join("libraries")),
        }
    }

    /// Converts a path at a native host boundary. Backend state retains only
    /// the resulting storage locator.
    pub fn native_path(path: impl AsRef<Path>) -> Self {
        Self::PlatformLocal(path.as_ref().to_string_lossy().into_owned())
    }
}

/// Validated startup resources. Services retain their own registry and
/// preference handles after startup.
#[derive(Clone, Debug)]
pub(crate) struct BackendContext {
    pub(crate) registry: LibraryRegistry,
    pub(crate) preferences: Arc<PreferenceStore>,
    pub(crate) app_data_dir: PathBuf,
}

impl BackendContext {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub fn initialize(location: AppDataLocation) -> Result<Self, String> {
        Self::open(location.data_dir()?, &location.default_library_root()?)
    }

    #[cfg(target_arch = "wasm32")]
    pub async fn initialize(location: AppDataLocation) -> Result<Self, String> {
        let app_data_dir = location.data_dir()?;
        let preferences = PreferenceStore::open(app_data_dir.join("preferences")).await.map_err(|error| error.to_string())?;
        Self::open_with_preferences(app_data_dir, &location.default_library_root()?, preferences)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn open(app_data_dir: PathBuf, library_root: &Path) -> Result<Self, String> {
        let preferences = PreferenceStore::open(app_data_dir.join("preferences")).map_err(|error| error.to_string())?;
        Self::open_with_preferences(app_data_dir, library_root, preferences)
    }

    fn open_with_preferences(app_data_dir: PathBuf, library_root: &Path, preferences: Arc<PreferenceStore>) -> Result<Self, String> {
        let registry = LibraryRegistry::open_with_library_root(app_data_dir.to_string_lossy(), library_root.to_string_lossy()).map_err(|error| format!("failed to initialize library registry: {error}"))?;
        #[cfg(target_os = "android")]
        client_platform_android::book::configure_lease_root(&app_data_dir).map_err(|error| error.to_string())?;
        Ok(Self { registry, preferences, app_data_dir })
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../../tests/app/data/tests.rs"]
mod tests;
