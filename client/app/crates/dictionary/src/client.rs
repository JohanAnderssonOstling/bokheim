//! Typed dictionary lookup with host-provided storage and lifecycle access.
use std::{future::Future, pin::Pin};

use crate::{DictionaryDownloads, DictionaryImport, DictionaryInfo, DictionaryLookup, DictionarySnapshot, Request, Response, Result};
use tokio::sync::watch;

/// UI calls run on the local executor; storage work stays behind the host client.
pub type ClientFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + 'a>>;

pub trait Client {
    fn request(&self, request: Request) -> ClientFuture<'_, Response>;
    fn import(&self, input: DictionaryImport) -> ClientFuture<'_, Vec<DictionaryInfo>>;
    fn changes(&self) -> ClientFuture<'_, watch::Receiver<()>>;

    fn list(&self) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(async move { self.request(Request::List).await?.into_dictionaries() })
    }

    fn snapshot(&self, languages: Vec<String>) -> ClientFuture<'_, DictionarySnapshot> {
        Box::pin(async move { self.request(Request::Snapshot { languages }).await?.into_snapshot() })
    }

    fn downloads(&self, languages: Vec<String>) -> ClientFuture<'_, DictionaryDownloads> {
        Box::pin(async move { self.request(Request::Downloads { languages }).await?.into_downloads() })
    }

    fn download(&self, id: String) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(async move { self.request(Request::Download { id }).await?.into_dictionaries() })
    }

    fn configure(&self, id: String, enabled: bool, direction: i8) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(async move { self.request(Request::Configure { id, enabled, direction }).await?.into_dictionaries() })
    }

    fn remove(&self, id: String) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(async move { self.request(Request::Remove { id }).await?.into_dictionaries() })
    }

    fn lookup(&self, query: String, languages: Vec<String>) -> ClientFuture<'_, DictionaryLookup>;
}
