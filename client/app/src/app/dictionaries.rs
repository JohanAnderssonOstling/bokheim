//! App wiring for the backend-owned dictionary service.
use super::{AppBackend, data::BackendContext};
use crate::BackendError;
#[cfg(target_arch = "wasm32")]
use client_platform_web::dictionary_files as files;
#[cfg(not(target_arch = "wasm32"))]
use dictionary::native as files;
use dictionary::{DictionaryFile, DictionaryInfo, DictionaryLookup, DictionaryStore as Store, Result as StorageResult, Storage};
use std::sync::Arc;

pub(crate) mod downloads;

#[derive(Clone)]
pub(super) struct AppStorage(BackendContext);
impl Storage for AppStorage {
    #[cfg(not(target_arch = "wasm32"))]
    type Source = std::fs::File;
    #[cfg(target_arch = "wasm32")]
    type Source = files::Source;
    fn read_value(&self, key: &str) -> StorageResult<Option<Vec<u8>>> {
        self.0.registry.read_value(key).map_err(|error| error.to_string())
    }
    fn write_value(&self, key: &str, bytes: &[u8]) -> StorageResult<()> {
        self.0.registry.write_value(key, bytes).map_err(|error| error.to_string())
    }
    async fn open(&self, id: &str, name: &str) -> StorageResult<Self::Source> {
        files::open(&self.0.app_data_dir, id, name).await
    }
    async fn write_all(&self, id: &str, components: Vec<(String, Vec<u8>)>) -> StorageResult<()> {
        files::write_all(&self.0.app_data_dir, id, components).await
    }
    async fn remove(&self, id: &str) -> StorageResult<()> {
        files::remove(&self.0.app_data_dir, id).await
    }
}

pub(super) type DictionaryStore = Arc<Store<AppStorage>>;

pub(super) fn new(context: BackendContext) -> DictionaryStore {
    Arc::new(Store::new(AppStorage(context)))
}

impl AppBackend {
    pub(crate) async fn dictionaries(&self) -> Result<Vec<DictionaryInfo>, BackendError> {
        self.state.dictionaries.list().await.map_err(BackendError::message)
    }
    pub(crate) async fn import_dictionary(&self, files: Vec<DictionaryFile>) -> Result<Vec<DictionaryInfo>, BackendError> {
        self.state.dictionaries.import(files).await.map_err(BackendError::message)
    }
    pub(crate) async fn configure_dictionary(&self, id: String, enabled: bool, direction: i8) -> Result<Vec<DictionaryInfo>, BackendError> {
        self.state.dictionaries.configure(id, enabled, direction).await.map_err(BackendError::message)
    }
    pub(crate) async fn remove_dictionary(&self, id: String) -> Result<Vec<DictionaryInfo>, BackendError> {
        self.state.dictionaries.remove(id).await.map_err(BackendError::message)
    }
    pub(crate) async fn lookup_dictionary(&self, query: String, languages: Vec<String>) -> Result<DictionaryLookup, BackendError> {
        self.state.dictionaries.lookup(query, languages).await.map_err(BackendError::message)
    }
}
