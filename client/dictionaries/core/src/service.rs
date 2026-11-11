//! Dictionary ownership and direct async access, shared by clients using one root.
use crate::{Client, ClientFuture, DictionaryDownloads, DictionaryImport, DictionaryInfo, DictionaryLookup, DictionarySnapshot, DictionaryStore, OperationResult as Result};
use client_platform_runtime::executor::BackendExecutor;
#[cfg(target_arch = "wasm32")]
use client_platform_runtime::file_storage::StorageLock;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{LazyLock, Mutex, PoisonError};
use std::{
    collections::HashMap,
    future::Future,
    path::PathBuf,
    sync::{Arc, Weak},
};
use tokio::sync::watch;
#[cfg(target_arch = "wasm32")]
use tokio::sync::OnceCell;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
static CLIENTS: LazyLock<Mutex<HashMap<PathBuf, Weak<Inner>>>> = LazyLock::new(Default::default);
#[cfg(target_arch = "wasm32")]
thread_local! { static CLIENTS: std::cell::RefCell<HashMap<PathBuf, Weak<Inner>>> = Default::default(); }
struct Inner {
    #[cfg(target_arch = "wasm32")]
    root: PathBuf,
    store: Arc<DictionaryStore>,
    executor: BackendExecutor,
    #[cfg(target_arch = "wasm32")]
    notifications: OnceCell<web::Notifications>,
}
#[derive(Clone)]
pub struct LocalClient(Arc<Inner>);
impl LocalClient {
    pub fn new(root: impl Into<PathBuf>, executor: BackendExecutor) -> Self {
        let root = root.into();
        let open = |clients: &mut HashMap<PathBuf, Weak<Inner>>| {
            clients.retain(|_, client| client.strong_count() > 0);
            if let Some(client) = clients.get(&root).and_then(Weak::upgrade) {
                return Self(client);
            }
            // Opening only allocates in-memory state; disk access remains lazy.
            let inner = Arc::new(Inner {
                store: Arc::new(DictionaryStore::open(&root)),
                #[cfg(target_arch = "wasm32")]
                root: root.clone(),
                executor,
                #[cfg(target_arch = "wasm32")]
                notifications: OnceCell::new(),
            });
            clients.insert(root, Arc::downgrade(&inner));
            Self(inner)
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            open(&mut CLIENTS.lock().unwrap_or_else(PoisonError::into_inner))
        }
        #[cfg(target_arch = "wasm32")]
        CLIENTS.with_borrow_mut(open)
    }
    async fn state(&self) -> Result<&Inner> {
        #[cfg(target_arch = "wasm32")]
        self.0.notifications.get_or_try_init(|| async { web::Notifications::new(&self.0.root, self.0.store.clone()) }).await?;
        Ok(&self.0)
    }
    async fn write<T, F: Future<Output = Result<T>>>(&self, operation: impl FnOnce(Arc<DictionaryStore>) -> F) -> Result<T> {
        let state = self.state().await?;
        state.executor.with_io_context(async {
            #[cfg(target_arch = "wasm32")]
            let _write = StorageLock::acquire(&state.root).await?;
            let outcome = operation(state.store.clone()).await;
            #[cfg(target_arch = "wasm32")]
            if let Some(notifications) = state.notifications.get() {
                notifications.publish();
            }
            outcome
        }).await
    }
}
impl Client for LocalClient {
    fn list(&self) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(async move {
            let state = self.state().await?;
            state.executor.with_io_context(state.store.list()).await
        })
    }
    fn snapshot(&self, languages: Vec<String>) -> ClientFuture<'_, DictionarySnapshot> {
        Box::pin(async move {
            let state = self.state().await?;
            state.executor.with_io_context(state.store.snapshot(&languages)).await
        })
    }
    fn downloads(&self, languages: Vec<String>) -> ClientFuture<'_, DictionaryDownloads> {
        Box::pin(async move {
            let state = self.state().await?;
            state.executor.with_io_context(state.store.downloads(&languages)).await
        })
    }
    fn download(&self, id: String) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(self.write(move |store| async move { store.download(id).await }))
    }
    fn configure(&self, id: String, enabled: bool, direction: i8) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(self.write(move |store| async move { store.configure(id, enabled, direction).await }))
    }
    fn remove(&self, id: String) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(self.write(move |store| async move { store.remove(id).await }))
    }
    fn import(&self, input: DictionaryImport) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(self.write(move |store| async move { store.import(input).await }))
    }
    fn changes(&self) -> ClientFuture<'_, watch::Receiver<()>> {
        Box::pin(async move { Ok(self.state().await?.store.changes()) })
    }
    fn lookup(&self, query: String, languages: Vec<String>) -> ClientFuture<'_, DictionaryLookup> {
        Box::pin(async move {
            let state = self.state().await?;
            state.executor.with_io_context(state.store.lookup(query, languages)).await
        })
    }
}
