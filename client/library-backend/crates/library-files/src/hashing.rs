//! Host-owned asynchronous hashing; browser work stays on the CPU worker.

use crate::asset_store::AssetStoreError;
use content_address::ContentHash;
use client_platform_runtime::executor::BoxedBackendFuture;
/// Incremental BLAKE3 hashing. Dropping a session must release worker resources.
pub trait HashSession {
    fn update<'a>(&'a mut self, bytes: &'a [u8]) -> BoxedBackendFuture<'a, Result<(), AssetStoreError>>;
    fn finish(self: Box<Self>) -> BoxedBackendFuture<'static, Result<ContentHash, AssetStoreError>>;
}
/// Starts independently owned sessions; the returned digest is the raw BLAKE3 content hash.
pub trait HashService: std::fmt::Debug + Send + Sync {
    fn start(&self) -> BoxedBackendFuture<'static, Result<Box<dyn HashSession>, AssetStoreError>>;
}
