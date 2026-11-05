//! Resolve user-selected destinations before the registry allocates library storage.
use super::LibraryService;
use crate::BackendError;
#[cfg(not(target_arch = "wasm32"))]
use app_preferences::library_folder;
impl LibraryService {
    pub(super) fn effective_library_root(&self) -> Result<String, BackendError> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(root) = library_folder::library_save_location(&self.config.preference_storage_root).map_err(BackendError::operation)? {
            return Ok(root);
        }
        Ok(self.config.registry.default_library_root().to_owned())
    }
}
