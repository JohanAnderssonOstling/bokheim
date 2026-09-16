//! Host-owned asynchronous hashing; browser work stays on the CPU worker.
use client_platform_runtime::executor::BoxedBackendFuture;
/// Incremental BLAKE3 hashing. Dropping a session must release worker resources.
pub trait HashSession {
    fn update<'a>(&'a mut self, bytes: &'a [u8]) -> BoxedBackendFuture<'a, Result<(), String>>;
    fn finish(self: Box<Self>) -> BoxedBackendFuture<'static, Result<String, String>>;
}
/// Starts independently owned sessions; the returned digest is hexadecimal BLAKE3.
pub trait HashService: std::fmt::Debug + Send + Sync {
    fn start(&self) -> BoxedBackendFuture<'static, Result<Box<dyn HashSession>, String>>;
}
