//! Device and synchronized application preferences.

#[cfg(not(feature = "kobo"))]
use client_platform_runtime::transfers;
pub use app_preferences::*;
use std::io;

const PREFERENCES_FILE: &str = "reader_preferences.json";
const BROWSING_PREFERENCES_FILE: &str = "browsing_preferences.json";

impl AppState {
    fn read_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> io::Result<Option<T>> {
        self.registry.read_value(key).map_err(|error| io::Error::other(error.to_string()))?.map(|bytes| serde_json::from_slice(&bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))).transpose()
    }

    /// Browsing preferences, or fresh ones if what is on disk cannot be read.
    ///
    /// Unlike the reader preferences below, this discards rather than fails
    /// closed. The two files hold different kinds of thing: a reader profile is
    /// someone's place in their books and their typography, and silently
    /// resetting it would lose work, so it stays and the load errors. This file
    /// is a theme, a font size and a view mode — a minute to set again — and it
    /// is the one whose shape changes when a theme is renamed or a preference
    /// retired. Failing closed on it means one stale enum value takes every
    /// other browsing preference down with it, and there is nothing here worth
    /// that.
    pub fn load_browsing_preferences(&self) -> io::Result<BrowsingPreferences> {
        match self.read_json(BROWSING_PREFERENCES_FILE) {
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                log::warn!("discarding unreadable browsing preferences: {error}");
                self.registry.remove_value(BROWSING_PREFERENCES_FILE).map_err(|error| io::Error::other(error.to_string()))?;
                Ok(BrowsingPreferences::default())
            }
            result => Ok(result?.unwrap_or_default()),
        }
    }

    pub fn apply_browsing_preference_patch(&self, patch: BrowsingPreferencePatch) -> io::Result<BrowsingPreferences> {
        let mut preferences = self.load_browsing_preferences()?;
        #[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
        let previous_metered = preferences.allow_metered_transfers();
        #[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
        let transfer_setting = matches!(patch, BrowsingPreferencePatch::AllowMeteredTransfers(_));
        patch.apply(&mut preferences).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let bytes = serde_json::to_vec_pretty(&preferences).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        #[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
        if transfer_setting { transfers::save_allow_metered(preferences.allow_metered_transfers())?; }
        if let Err(error) = self.registry.write_value(BROWSING_PREFERENCES_FILE, &bytes) {
            #[cfg(all(not(target_arch = "wasm32"), not(feature = "kobo")))]
            if transfer_setting { let _ = transfers::save_allow_metered(previous_metered); }
            return Err(io::Error::other(error.to_string()));
        }
        #[cfg(target_arch = "wasm32")]
        transfers::POLICY.set_allow_metered(preferences.allow_metered_transfers());
        client_platform_runtime::text_search::set_enabled(preferences.text_search());
        Ok(preferences)
    }

    pub fn load_reader_preferences(&self) -> io::Result<ReaderPreferences> {
        self.registry
            .read_value(PREFERENCES_FILE)
            .map_err(|error| io::Error::other(error.to_string()))?
            .map(|bytes| serde_json::from_slice(&bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)))
            .transpose()
            .map(|preferences| preferences.unwrap_or_default())
    }

    pub fn apply_reader_preference_patches(&self, patches: Vec<ReaderPreferencePatch>) -> io::Result<ReaderPreferences> {
        let mut preferences = self.load_reader_preferences()?;
        for patch in patches {
            patch.apply(&mut preferences).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        }
        let bytes = serde_json::to_vec_pretty(&preferences).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        self.registry.write_value(PREFERENCES_FILE, &bytes).map_err(|error| io::Error::other(error.to_string()))?;
        Ok(preferences)
    }
}
use super::core::*;

impl AppBackend {
    pub(crate) fn default_library_save_location(&self) -> Result<Option<String>, BackendError> {
        self.state.registry.default_library_save_location().map_err(BackendError::operation)
    }
    pub(crate) fn set_default_library_save_location(&self, path: String) -> Result<String, BackendError> {
        self.state.registry.set_default_library_save_location(&path).map_err(BackendError::operation)
    }

    pub(crate) fn reader_preferences(&self) -> Result<ReaderPreferences, BackendError> {
        self.state.load_reader_preferences().map_err(BackendError::operation)
    }

    pub(crate) fn browsing_preferences(&self) -> Result<BrowsingPreferences, BackendError> {
        self.state.load_browsing_preferences().map_err(BackendError::operation)
    }

    pub(crate) fn update_browsing_preference(&self, patch: BrowsingPreferencePatch) -> Result<BrowsingPreferences, BackendError> {
        let clear_text_index = matches!(patch, BrowsingPreferencePatch::TextSearch(false));
        let preferences = self.state.apply_browsing_preference_patch(patch).map_err(BackendError::operation)?;
        if clear_text_index {
            let backend = self.clone();
            self.state.executor.spawn_detached(async move {
                // Include registered libraries that could not open their source
                // folder; cleanup must not depend on Settings remaining visible.
                if let Err(error) = backend.text_search_index_bytes().await {
                    log::warn!("could not finish removing text search indexes: {error}");
                }
            });
        }
        Ok(preferences)
    }

    pub(crate) fn update_reader_preferences(&self, patches: Vec<ReaderPreferencePatch>) -> Result<ReaderPreferences, BackendError> {
        self.state.apply_reader_preference_patches(patches).map_err(BackendError::operation)
    }
}

impl AppBackend {
    /// Device-local export preference; it never enters library synchronization.
    pub(crate) fn default_annotation_export_folder(&self) -> Result<Option<String>, BackendError> {
        if let Some(folder)=self.state.read_json::<String>("annotation-export-folder:v1").map_err(BackendError::operation)? { return Ok(Some(folder)); }
        #[cfg(not(target_arch = "wasm32"))]
        { return Ok(Some(client_platform_desktop::default_annotation_export_folder().map_err(BackendError::message)?.to_string_lossy().into_owned())); }
        #[cfg(target_arch = "wasm32")]
        Ok(None)
    }
    pub(crate) fn set_default_annotation_export_folder(&self,folder:String)->Result<String,BackendError> {
        if folder.trim().is_empty() || !std::path::Path::new(&folder).is_absolute() || std::path::Path::new(&folder).components().any(|component|matches!(component,std::path::Component::ParentDir)) { return Err(BackendError::message("choose an absolute annotation export folder")); }
        self.state.registry.write_value("annotation-export-folder:v1",&serde_json::to_vec(&folder).map_err(BackendError::operation)?).map_err(BackendError::operation)?;
        Ok(folder)
    }
}

#[cfg(all(test,not(target_arch="wasm32")))]
mod annotation_export_tests {
    use super::*;
    use crate::app::data::{AppDataLocation,BackendContext};
    #[test]
    fn annotation_export_folder_is_device_local_and_survives_reopening() {
        let root=tempfile::tempdir().unwrap();
        let context=BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
        let backend=AppBackend::account_test_backend(context,"http://127.0.0.1:1".parse().unwrap());
        let folder=root.path().join("chosen-export-folder").to_string_lossy().into_owned();
        assert!(backend.set_default_annotation_export_folder("relative/folder".into()).is_err());
        backend.set_default_annotation_export_folder(folder.clone()).unwrap();
        assert_eq!(backend.default_annotation_export_folder().unwrap(),Some(folder.clone()));
        assert!(!std::path::Path::new(&folder).exists(),"choosing a preference should not create an output directory");
        drop(backend);
        let context=BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
        let backend=AppBackend::account_test_backend(context,"http://127.0.0.1:1".parse().unwrap());
        assert_eq!(backend.default_annotation_export_folder().unwrap(),Some(folder));
    }
}
