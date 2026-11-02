//! Frontend access to a dictionary service. Hosts provide the worker connection.
use std::{future::Future, pin::Pin};

use crate::{DictionaryDownloads, DictionaryFile, DictionaryInfo, DictionaryLookup, Result};
use tokio::sync::watch;

/// UI calls run on the local executor; storage work stays behind the host client.
pub type ClientFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + 'a>>;

pub trait Client {
    fn list(&self) -> ClientFuture<'_, Vec<DictionaryInfo>>;
    fn downloads(&self, languages: Vec<String>) -> ClientFuture<'_, DictionaryDownloads>;
    fn import(&self, files: Vec<DictionaryFile>) -> ClientFuture<'_, Vec<DictionaryInfo>>;
    fn download(&self, id: String) -> ClientFuture<'_, Vec<DictionaryInfo>>;
    fn configure(&self, id: String, enabled: bool, direction: i8) -> ClientFuture<'_, Vec<DictionaryInfo>>;
    fn remove(&self, id: String) -> ClientFuture<'_, Vec<DictionaryInfo>>;
    fn lookup(&self, query: String, languages: Vec<String>) -> ClientFuture<'_, DictionaryLookup>;
    fn changes(&self) -> ClientFuture<'_, watch::Receiver<()>>;
}
