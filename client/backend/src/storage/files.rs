//! Host adapters for library-owned filesystem services.
#[cfg(target_arch = "wasm32")]
use crate::executor::BoxedBackendFuture;
pub(crate) use library_files::asset_store::*;

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
    fn start(&self) -> BoxedBackendFuture<'static, Result<Box<dyn library_files::hashing::HashSession>, String>> {
        Box::pin(async { Ok(Box::new(crate::worker_cpu::RemoteHasher::new().await?) as Box<dyn library_files::hashing::HashSession>) })
    }
}
#[cfg(target_arch = "wasm32")]
impl library_files::hashing::HashSession for crate::worker_cpu::RemoteHasher {
    fn update<'a>(&'a mut self, bytes: &'a [u8]) -> BoxedBackendFuture<'a, Result<(), String>> {
        Box::pin(crate::worker_cpu::RemoteHasher::update(self, bytes))
    }
    fn finish(self: Box<Self>) -> BoxedBackendFuture<'static, Result<String, String>> {
        Box::pin(crate::worker_cpu::RemoteHasher::finish(*self))
    }
}
