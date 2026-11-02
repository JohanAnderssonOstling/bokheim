//! App-facing commands for explicit dictionary downloads.
use super::{AppBackend, BackendError, DictionaryInfo};
use dictionary::DictionaryDownloads;

impl AppBackend {
    pub(crate) async fn dictionary_downloads(&self, languages: Vec<String>) -> Result<DictionaryDownloads, BackendError> {
        self.state.dictionaries.downloads(&languages).await.map_err(BackendError::message)
    }
    pub(crate) async fn download_dictionary(&self, id: String) -> Result<Vec<DictionaryInfo>, BackendError> {
        self.state.dictionaries.download(id).await.map_err(BackendError::message)
    }
    pub(crate) fn dictionary_changes(&self) -> tokio::sync::watch::Receiver<()> {
        self.state.dictionaries.changes()
    }
}
