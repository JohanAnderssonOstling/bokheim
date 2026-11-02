//! Standalone dictionary ownership and scheduling, shared by clients using one root.
use crate::{Client, ClientFuture, DictionaryImport, DictionaryInfo, DictionaryLookup, DictionaryStore, Request, Response, Result};
use client_platform_runtime::executor::BackendExecutor;
#[cfg(target_arch = "wasm32")]
use client_platform_runtime::file_storage::StorageLock;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{LazyLock, Mutex, PoisonError};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Weak},
};
use tokio::sync::{OnceCell, oneshot, watch};
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(not(target_arch = "wasm32"))]
static CLIENTS: LazyLock<Mutex<HashMap<PathBuf, Weak<Inner>>>> = LazyLock::new(Default::default);
#[cfg(target_arch = "wasm32")]
thread_local! { static CLIENTS: std::cell::RefCell<HashMap<PathBuf, Weak<Inner>>> = Default::default(); }
struct Inner {
    root: PathBuf,
    state: OnceCell<Arc<State>>,
}
struct State {
    store: Arc<DictionaryStore>,
    executor: OnceCell<BackendExecutor>,
    #[cfg(target_arch = "wasm32")]
    notifications: web::Notifications,
}
impl State {
    async fn executor(&self) -> Result<&BackendExecutor> {
        self.executor.get_or_try_init(|| async { BackendExecutor::new().map_err(|error| error.to_string()) }).await
    }
}
#[derive(Clone)]
pub struct LocalClient(Arc<Inner>);
impl LocalClient {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let open = |clients: &mut HashMap<PathBuf, Weak<Inner>>| {
            clients.retain(|_, client| client.strong_count() > 0);
            if let Some(client) = clients.get(&root).and_then(Weak::upgrade) {
                return Self(client);
            }
            let inner = Arc::new(Inner { root: root.clone(), state: OnceCell::new() });
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
    async fn state(&self) -> Result<Arc<State>> {
        self.0
            .state
            .get_or_try_init(|| async {
                let store = Arc::new(DictionaryStore::open(&self.0.root));
                #[cfg(target_arch = "wasm32")]
                let notifications = web::Notifications::new(&self.0.root, store.clone())?;
                Ok(Arc::new(State {
                    store,
                    executor: OnceCell::new(),
                    #[cfg(target_arch = "wasm32")]
                    notifications,
                }))
            })
            .await
            .cloned()
    }
}
impl Client for LocalClient {
    fn request(&self, request: Request) -> ClientFuture<'_, Response> {
        Box::pin(async move {
            let state = self.state().await?;
            if matches!(request, Request::List | Request::Snapshot { .. } | Request::Downloads { .. }) {
                return state.store.execute(request).await;
            }
            let operation = state.clone();
            let owner = self.0.clone();
            #[cfg(target_arch = "wasm32")]
            let root = self.0.root.clone();
            let (reply, result) = oneshot::channel();
            // Lifecycle work owns its write through completion even if the UI
            // closes while an asynchronous OPFS write is committing.
            state.executor().await?.spawn_detached(async move {
                let _owner = owner;
                if reply.is_closed() {
                    return;
                }
                let result = async move {
                    #[cfg(target_arch = "wasm32")]
                    let _write = StorageLock::acquire(&root).await?;
                    let result = operation.store.execute(request).await;
                    #[cfg(target_arch = "wasm32")]
                    operation.notifications.publish();
                    result
                }
                .await;
                let _ = reply.send(result);
            });
            result.await.map_err(|_| "Dictionary operation stopped before replying".to_owned())?
        })
    }
    fn import(&self, input: DictionaryImport) -> ClientFuture<'_, Vec<DictionaryInfo>> {
        Box::pin(async move {
            let state = self.state().await?;
            let operation = state.clone();
            let owner = self.0.clone();
            #[cfg(target_arch = "wasm32")]
            let root = self.0.root.clone();
            let (reply, result) = oneshot::channel();
            state.executor().await?.spawn_detached(async move {
                let _owner = owner;
                if reply.is_closed() {
                    return;
                }
                let result = async move {
                    #[cfg(target_arch = "wasm32")]
                    let _write = StorageLock::acquire(&root).await?;
                    let result = operation.store.import(input).await;
                    #[cfg(target_arch = "wasm32")]
                    operation.notifications.publish();
                    result
                }
                .await;
                let _ = reply.send(result);
            });
            result.await.map_err(|_| "Dictionary import stopped before replying".to_owned())?
        })
    }
    fn changes(&self) -> ClientFuture<'_, watch::Receiver<()>> {
        Box::pin(async move { Ok(self.state().await?.store.changes()) })
    }
    fn lookup(&self, query: String, languages: Vec<String>) -> ClientFuture<'_, DictionaryLookup> {
        Box::pin(async move {
            let state = self.state().await?;
            state.store.lookup(query, languages).await
        })
    }
}
