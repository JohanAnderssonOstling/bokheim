//! Supply Tokio's I/O context when HTTP futures are polled by a native UI executor.
use std::future::Future;
#[cfg(not(target_arch = "wasm32"))]
use std::future::poll_fn;
use std::io;
#[cfg(not(target_arch = "wasm32"))]
use std::{pin::pin, sync::OnceLock};
#[cfg(not(target_arch = "wasm32"))]
use tokio::runtime::{Builder, Handle, Runtime};

#[cfg(not(target_arch = "wasm32"))]
fn handle() -> io::Result<Handle> {
    if let Ok(handle) = Handle::try_current() {
        return Ok(handle);
    }
    static RUNTIME: OnceLock<io::Result<Runtime>> = OnceLock::new();
    let runtime = RUNTIME.get_or_init(|| Builder::new_multi_thread().worker_threads(1).thread_name("bokheim-http").enable_all().build());
    let runtime = runtime.as_ref().map_err(|error| io::Error::new(error.kind(), error.to_string()))?;
    Ok(runtime.handle().clone())
}

/// Construct HTTP clients and requests in a native I/O context; browsers call directly.
pub fn with_runtime<T>(operation: impl FnOnce() -> T) -> io::Result<T> {
    #[cfg(not(target_arch = "wasm32"))]
    let handle = handle()?;
    #[cfg(not(target_arch = "wasm32"))]
    let _entered = handle.enter();
    Ok(operation())
}

/// Poll an HTTP operation, including response-body reads, with native I/O available.
/// Browser operations are awaited directly. Dropping the future cancels the operation.
pub async fn run<T>(future: impl Future<Output = T>) -> io::Result<T> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let handle = handle()?;
        let mut future = pin!(future);
        Ok(poll_fn(|context| {
            // Enter only for this poll; the guard must never survive an await.
            let _entered = handle.enter();
            future.as_mut().poll(context)
        })
        .await)
    }
    #[cfg(target_arch = "wasm32")]
    Ok(future.await)
}
