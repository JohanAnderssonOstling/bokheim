//! Direct preference access, shared by windows using the same storage root.
use crate::{BrowsingPreferencePatch, BrowsingPreferences, PreferenceStore, ReaderPreferencePatch, ReaderPreferences};
use client_platform_runtime::executor::BackendExecutor;
use std::{future::Future, io, path::PathBuf, sync::Arc};
use tokio::sync::OnceCell;

#[derive(Clone)]
pub struct Client {
    pub(crate) root: PathBuf,
    store: Arc<OnceCell<Arc<PreferenceStore>>>,
    host: Option<Host>,
}

#[derive(Clone)]
struct Host {
    executor: BackendExecutor,
    _lifetime: Arc<dyn Send + Sync>,
}

impl Client {
    /// Opens device-local preference documents.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), store: Arc::new(OnceCell::new()), host: None }
    }

    pub fn local(store: Arc<PreferenceStore>, executor: BackendExecutor, host: impl Send + Sync + 'static) -> Self {
        Self { root: store.storage_root().to_owned(), store: Arc::new(OnceCell::new_with(Some(store))), host: Some(Host { executor, _lifetime: Arc::new(host) }) }
    }

    async fn save<T>(&self, save: impl Future<Output = io::Result<T>>) -> io::Result<T> {
        match &self.host {
            Some(host) => host.executor.with_io_context(save).await,
            None => save.await,
        }
    }

    async fn store(&self) -> io::Result<&Arc<PreferenceStore>> {
        self.store
            .get_or_try_init(|| async {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    use client_platform_runtime::executor::run_blocking;
                    let root = self.root.clone();
                    run_blocking(move || PreferenceStore::open(root)).await.map_err(io::Error::other)?
                }
                #[cfg(target_arch = "wasm32")]
                PreferenceStore::open(self.root.clone()).await
            })
            .await
    }

    pub async fn reader(&self) -> io::Result<ReaderPreferences> {
        Ok(self.store().await?.reader())
    }

    pub async fn browsing(&self) -> io::Result<BrowsingPreferences> {
        Ok(self.store().await?.browsing())
    }

    pub async fn update_reader(&self, patches: Vec<ReaderPreferencePatch>) -> io::Result<ReaderPreferences> {
        if patches.is_empty() {
            return self.reader().await;
        }
        self.save(self.store().await?.update_reader(patches)).await
    }

    pub async fn update_browsing(&self, patch: BrowsingPreferencePatch) -> io::Result<BrowsingPreferences> {
        self.save(self.store().await?.update_browsing(patch)).await
    }

    pub async fn default_annotation_export_folder(&self) -> io::Result<Option<String>> {
        if let Some(folder) = self.store().await?.annotation_export_folder() {
            return Ok(Some(folder));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let documents = dirs::document_dir().or_else(|| dirs::home_dir().map(|home| home.join("Documents"))).ok_or_else(|| io::Error::other("could not locate the Documents directory"))?;
            Ok(Some(documents.join("Bokheim annotations").to_string_lossy().into_owned()))
        }
        #[cfg(target_arch = "wasm32")]
        Ok(None)
    }

    pub async fn set_default_annotation_export_folder(&self, folder: String) -> io::Result<String> {
        self.save(self.store().await?.set_annotation_export_folder(folder)).await
    }
}
