use super::task::{owned_task, BackendTask};
use super::{BackendExecutorError, BackendFuture, BackendOutput, BackendSend, BackendTimeout};
use std::future::Future;
use std::sync::Arc;

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

    pub fn spawn<T: BackendOutput>(&self, future: impl BackendFuture<T>) -> BackendTask<T> {
        let (task, runner) = owned_task(future);
        self.inner.handle.spawn(runner);
        task
    }

    pub fn spawn_detached(&self, future: impl BackendFuture<()>) {
        self.inner.handle.spawn(future);
    }

    pub fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        // Synchronous backend entry points bridge into the backend-owned runtime.
        self.inner.handle.block_on(future)
    }
}

pub async fn run_blocking<T: BackendOutput>(operation: impl FnOnce() -> T + BackendSend + 'static) -> Result<T, BackendExecutorError> {
    tokio::task::spawn_blocking(operation).await.map_err(|error| BackendExecutorError(format!("backend blocking task failed: {error}")))
}

pub fn spawn_detached(future: impl BackendFuture<()>) {
    tokio::spawn(future);
}

pub async fn sleep(duration: std::time::Duration) {
    tokio::time::sleep(duration).await;
}

pub async fn timeout<T>(duration: std::time::Duration, future: impl Future<Output = T>) -> Result<T, BackendTimeout> {
    tokio::time::timeout(duration, future).await.map_err(|_| BackendTimeout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    fn assert_send<T: Send>() {}

    #[test]
    fn executor_reuses_one_runtime_for_spawn_and_block_on() {
        let executor = BackendExecutor::new().unwrap();
        assert_send::<BackendTask<()>>();
        let first = executor.spawn(async {
            tokio::time::sleep(Duration::from_millis(1)).await;
            41_u8
        });
        assert_eq!(executor.block_on(first).unwrap(), 41);
        assert_eq!(
            executor.block_on(async {
                tokio::time::sleep(Duration::from_millis(1)).await;
                42_u8
            }),
            42
        );
    }

    #[test]
    fn dropping_backend_task_cancels_its_future() {
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
            }
        }

        let executor = BackendExecutor::new().unwrap();
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = DropFlag(dropped.clone());
        let task = executor.spawn(async move {
            let _flag = flag;
            std::future::pending::<()>().await
        });
        std::thread::sleep(Duration::from_millis(10));
        drop(task);
        for _ in 0..100 {
            if dropped.load(Ordering::Acquire) {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("cancelled backend future was not dropped");
    }

    #[test]
    fn task_panics_become_errors_at_the_host_boundary() {
        let executor = BackendExecutor::new().unwrap();
        let task = executor.spawn(async { panic!("broken backend operation") });
        let error = executor.block_on(task).unwrap_err();
        assert!(error.to_string().contains("panicked"));
    }
}

/// Report available CPU capacity; callers choose their own concurrency limits.
pub fn available_parallelism() -> usize {
    std::thread::available_parallelism().map(|count| count.get()).unwrap_or(1)
}
