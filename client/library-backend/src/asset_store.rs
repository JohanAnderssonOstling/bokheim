//! Host adapters for library-owned filesystem services.
#[cfg(target_arch = "wasm32")]
use crate::executor::BoxedBackendFuture;
pub use library_files::asset_store::*;

pub(crate) fn open(locator: &str) -> Result<AssetStore, AssetStoreError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        AssetStore::open(locator)
    }
    #[cfg(target_arch = "wasm32")]
    {
        AssetStore::open(locator, std::sync::Arc::new(WorkerHashService))
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Debug)]
struct WorkerHashService;
#[cfg(target_arch = "wasm32")]
impl library_files::hashing::HashService for WorkerHashService {
    fn start(&self) -> BoxedBackendFuture<'static, Result<Box<dyn library_files::hashing::HashSession>, AssetStoreError>> {
        Box::pin(async {
            let session = crate::worker_cpu::RemoteHashSession::new(&crate::worker_cpu::cpu_host()).await.map_err(AssetStoreError::operation)?;
            Ok(Box::new(WorkerHashSession(session)) as Box<dyn library_files::hashing::HashSession>)
        })
    }
}
#[cfg(target_arch = "wasm32")]
struct WorkerHashSession(crate::worker_cpu::RemoteHashSession);
#[cfg(target_arch = "wasm32")]
impl library_files::hashing::HashSession for WorkerHashSession {
    fn update<'a>(&'a mut self, bytes: &'a [u8]) -> BoxedBackendFuture<'a, Result<(), AssetStoreError>> {
        Box::pin(async move { self.0.update(bytes).await.map_err(AssetStoreError::operation) })
    }
    fn finish(self: Box<Self>) -> BoxedBackendFuture<'static, Result<String, AssetStoreError>> {
        Box::pin(async move { self.0.finish().await.map_err(AssetStoreError::operation) })
    }
}
