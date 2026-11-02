//! Connect the dictionary crate's frontend client to the app worker.
use super::AppClient;
use crate::WikipediaLookup;
use crate::runtime::AppCommand;
use dictionary::{Client, ClientFuture, DictionaryDownloads, DictionaryFile, DictionaryInfo, DictionaryLookup};

impl Client for AppClient {
    fn list(&self) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(async move { self.request(AppCommand::Dictionaries).await.map_err(|error| error.to_string()) })
    }
    fn downloads(&self, languages: Vec<String>) -> ClientFuture<'_, DictionaryDownloads> {
        Box::pin(async move { self.request(AppCommand::DictionaryDownloads { languages }).await.map_err(|error| error.to_string()) })
    }
    fn import(&self, files: Vec<DictionaryFile>) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(async move { self.request(AppCommand::ImportDictionary { files }).await.map_err(|error| error.to_string()) })
    }
    fn download(&self, id: String) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(async move { self.request(AppCommand::DownloadDictionary { id }).await.map_err(|error| error.to_string()) })
    }
    fn configure(&self, id: String, enabled: bool, direction: i8) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(async move { self.request(AppCommand::ConfigureDictionary { id, enabled, direction }).await.map_err(|error| error.to_string()) })
    }
    fn remove(&self, id: String) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(async move { self.request(AppCommand::RemoveDictionary { id }).await.map_err(|error| error.to_string()) })
    }
    fn lookup(&self, query: String, languages: Vec<String>) -> ClientFuture<'_, DictionaryLookup> {
        Box::pin(async move { self.request(AppCommand::LookupDictionary { query, languages }).await.map_err(|error| error.to_string()) })
    }
    fn changes(&self) -> ClientFuture<'_, tokio::sync::watch::Receiver<()>> {
        Box::pin(async move { self.transport.dictionary_changes_transport().await.map_err(|error| error.to_string()) })
    }
}

impl AppClient {
    /// Wikipedia is optional: offline, missing pages and transport failures
    /// all finish quietly. The caller must not retry a failed lookup.
    pub async fn lookup_wikipedia(&self, query: String, languages: Vec<String>) -> Option<WikipediaLookup> {
        self.request(AppCommand::LookupWikipedia { query, languages }).await.ok().flatten()
    }
}
