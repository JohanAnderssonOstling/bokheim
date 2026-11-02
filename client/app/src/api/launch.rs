//! Platform startup configuration; running operations go through client handles.

use crate::app::AppDataLocation;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::BackendContext;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct BackendLaunch {
    data_dir: PathBuf,
    default_library_root: PathBuf,
    discovery: Option<(String, PathBuf)>,
    private_library_storage: bool,
    pub(crate) libraries_after_first_frame: bool,
}

impl BackendLaunch {
    pub fn initialize(location: AppDataLocation) -> Result<Self, String> {
        let default_library_root = location.default_library_root()?;
        let data_dir = location.data_dir()?;
        // Desktop startup opens instance and update locks before the backend
        // registry, which otherwise creates this directory on first launch.
        fs::create_dir_all(&data_dir).map_err(|error| format!("failed to create app-data directory {}: {error}", data_dir.display()))?;
        Ok(Self { data_dir, default_library_root, discovery: None, private_library_storage: false, libraries_after_first_frame: false })
    }

    /// Location for platform-owned files such as desktop instance locks and UI sessions.
    pub fn app_data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Put future libraries in app-owned storage, including after switching
    /// from a distribution that allowed a shared-storage default. Existing
    /// registered libraries and their files are never moved or deleted.
    pub fn with_private_library_storage(mut self) -> Self {
        self.default_library_root = self.data_dir.join("libraries");
        self.private_library_storage = true;
        self
    }

    /// Use this root for discovery and future libraries, registering a fallback
    /// only when none exist. Saved locations continue to take precedence.
    pub fn with_library_discovery(mut self, root: PathBuf, fallback_name: String, fallback_path: PathBuf) -> Self {
        self.default_library_root = root;
        self.discovery = Some((fallback_name, fallback_path));
        self
    }

    /// Build the initial UI from registry metadata, then open all libraries on
    /// the backend worker. The host must hold `defer_library_maintenance()`
    /// until its first frame; early requests wait for opening to finish.
    pub fn with_libraries_after_first_frame(mut self) -> Self {
        self.libraries_after_first_frame = true;
        self
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn into_context(self) -> Result<BackendContext, String> {
        let context = BackendContext::open(self.data_dir, &self.default_library_root)?;
        if self.private_library_storage {
            app_preferences::library_folder::set_library_save_location(context.preferences.storage_root(), &self.default_library_root.to_string_lossy()).map_err(|error| error.to_string())?;
        }
        if let Some((name, fallback)) = self.discovery {
            context.registry.discover(&self.default_library_root).map_err(|error| error.to_string())?;
            if context.registry.entries().map_err(|error| error.to_string())?.is_empty() && context.registry.pending_local_purges().map_err(|error| error.to_string())?.is_empty() {
                context.registry.create(&name, Some(fallback.to_string_lossy().into_owned())).map_err(|error| error.to_string())?;
            }
        }
        Ok(context)
    }
}

#[cfg(test)]
#[path = "../../tests/api/launch/tests.rs"]
mod tests;
