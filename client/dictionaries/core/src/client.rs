//! Typed dictionary lookup with host-provided storage and lifecycle access.
use std::{future::Future, pin::Pin};

use crate::{DictionaryDownloads, DictionaryImport, DictionaryInfo, DictionaryLookup, DictionarySnapshot, Result};
use tokio::sync::watch;

/// UI calls run on the local executor; storage work stays behind the host client.
pub type ClientFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + 'a>>;

pub trait Client {
    fn import(&self, input: DictionaryImport) -> ClientFuture<'_, Vec<DictionaryInfo>>;
    fn changes(&self) -> ClientFuture<'_, watch::Receiver<()>>;
    fn list(&self) -> ClientFuture<'_, Vec<DictionaryInfo>>;
    fn snapshot(&self, languages: Vec<String>) -> ClientFuture<'_, DictionarySnapshot>;
    fn downloads(&self, languages: Vec<String>) -> ClientFuture<'_, DictionaryDownloads>;
    fn download(&self, id: String) -> ClientFuture<'_, Vec<DictionaryInfo>>;
    fn configure(&self, id: String, enabled: bool, direction: i8) -> ClientFuture<'_, Vec<DictionaryInfo>>;
    fn remove(&self, id: String) -> ClientFuture<'_, Vec<DictionaryInfo>>;
    fn lookup(&self, query: String, languages: Vec<String>) -> ClientFuture<'_, DictionaryLookup>;
}
