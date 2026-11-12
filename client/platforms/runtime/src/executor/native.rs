use super::startup::startup_ready;
use super::task::{owned_task, BackendTask};
use super::{BackendExecutorError, BackendFuture, BackendOutput, BackendSend, BackendTimeout};
use std::future::{Future, poll_fn};
use std::sync::Arc;
use std::thread;

/// Native async-I/O execution boundary owned by the backend.
#[derive(Clone)]
pub struct BackendExecutor {
    inner: Arc<BackendExecutorInner>,
}

struct BackendExecutorInner {
    handle: tokio::runtime::Handle,
    runtime: Option<tokio::runtime::Runtime>,
}

impl Drop for BackendExecutorInner {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl BackendExecutor {
    pub fn new() -> Result<Self, BackendExecutorError> {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name("bokheim-async").enable_all().build().map_err(|error| BackendExecutorError(error.to_string()))?;
        let handle = runtime.handle().clone();
        Ok(Self { inner: Arc::new(BackendExecutorInner { handle, runtime: Some(runtime) }) })
    }

    /// Poll on the caller's task while providing Tokio's I/O and blocking pool.
    /// The runtime guard is released after every poll, before any suspension.
    pub async fn with_io_context<T>(&self, future: impl Future<Output = T>) -> T {
        futures_util::pin_mut!(future);
        poll_fn(|cx| {
            let _runtime = self.inner.handle.enter();
            future.as_mut().poll(cx)
        })
        .await
    }

    pub fn spawn<T: BackendOutput>(&self, future: impl BackendFuture<T>) -> BackendTask<T> {
        let (task, runner) = owned_task(future);
        self.inner.handle.spawn(async move {
            startup_ready().await;
            runner.await
        });
        task
    }

    pub fn spawn_detached(&self, future: impl BackendFuture<()>) {
        self.inner.handle.spawn(async move {
            startup_ready().await;
            future.await
        });
    }

    /// Runs synchronous storage work on this runtime's blocking pool.
    pub fn dispatch_storage(&self, operation: impl FnOnce() + Send + 'static) -> Result<(), BackendExecutorError> {
        let executor = self.clone();
        self.inner.handle.spawn_blocking(move || {
            let _executor = executor;
            crate::profiling::operation(operation);
        });
        Ok(())
    }

    pub fn storage_cleanup(&self, operation: impl FnOnce() + Send + 'static) {
        let _ = self.dispatch_storage(operation);
    }

    pub fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        // Synchronous backend entry points bridge into the backend-owned runtime.
        self.inner.handle.block_on(future)
    }
}

pub async fn run_blocking<T: BackendOutput>(operation: impl FnOnce() -> T + BackendSend + 'static) -> Result<T, BackendExecutorError> {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => handle.spawn_blocking(move || crate::profiling::operation(operation)).await.map_err(|error| BackendExecutorError(format!("backend blocking task failed: {error}"))),
        Err(_) => crate::storage_queue::run(operation).await.map_err(BackendExecutorError),
    }
}

pub fn spawn_detached(future: impl BackendFuture<()>) {
    tokio::spawn(async move {
        startup_ready().await;
        future.await
    });
}

pub async fn sleep(duration: std::time::Duration) -> Result<(), BackendExecutorError> {
    tokio::time::sleep(duration).await;
    Ok(())
}

pub async fn timeout<T>(duration: std::time::Duration, future: impl Future<Output = T>) -> Result<T, BackendTimeout> {
    tokio::time::timeout(duration, future).await.map_err(|_| BackendTimeout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::defer_background_work;
    use std::time::Duration;

    #[test]
    fn startup_gate_prevents_background_side_effects_until_commit() {
        let permit = defer_background_work();
        let executor = BackendExecutor::new().unwrap();
        let (sent, received) = std::sync::mpsc::channel();
        executor.spawn_detached(async move {
            sent.send(()).unwrap();
        });
        assert!(received.recv_timeout(Duration::from_millis(30)).is_err());
        permit.release();
        received.recv_timeout(Duration::from_secs(2)).expect("background work resumes after commit");
    }
}

/// Report available CPU capacity; callers choose their own concurrency limits.
pub fn available_parallelism() -> usize {
    thread::available_parallelism().map(|count| count.get()).unwrap_or(1)
}
