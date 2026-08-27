use super::{BackendBlockingThunk, BackendExecutorError, BackendFuture, BackendOutput};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

/// Native async-I/O execution boundary owned by the backend.
#[derive(Clone)]
pub struct BackendExecutor {
    inner: Arc<BackendExecutorInner>,
}

struct BackendExecutorInner {
    handle: tokio::runtime::Handle,
    _runtime: OwnedRuntime,
}

struct OwnedRuntime(Mutex<Option<tokio::runtime::Runtime>>);

impl Drop for OwnedRuntime {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.get_mut().ok().and_then(Option::take) {
            runtime.shutdown_background();
        }
    }
}

pub struct BackendTask<T> {
    task: tokio::task::JoinHandle<T>,
}

impl<T> Future for BackendTask<T> {
    type Output = Result<T, BackendExecutorError>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().task).poll(context).map(|result| result.map_err(|error| BackendExecutorError(format!("backend task failed: {error}"))))
    }
}

impl<T> Drop for BackendTask<T> {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl BackendExecutor {
    pub fn new() -> Result<Self, BackendExecutorError> {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).thread_name(format!("{}-async", crate::APP_NAME_LOWER)).enable_all().build().map_err(|error| BackendExecutorError(error.to_string()))?;
        let handle = runtime.handle().clone();
        Ok(Self { inner: Arc::new(BackendExecutorInner { handle, _runtime: OwnedRuntime(Mutex::new(Some(runtime))) }) })
    }

    pub fn spawn<T: BackendOutput>(&self, future: impl BackendFuture<T>) -> BackendTask<T> {
        BackendTask { task: self.inner.handle.spawn(future) }
    }

    pub fn spawn_detached(&self, future: impl BackendFuture<()>) {
        self.inner.handle.spawn(future);
    }

    pub fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        // Synchronous backend entry points bridge into the backend-owned runtime.
        self.inner.handle.block_on(future)
    }
}

pub async fn run_blocking<T, F>(operation: F) -> Result<T, BackendExecutorError>
where
    T: BackendOutput,
    F: BackendBlockingThunk<T>,
{
    tokio::task::spawn_blocking(operation).await.map_err(|error| BackendExecutorError(format!("backend blocking task failed: {error}")))
}

pub(crate) fn spawn_detached(future: impl BackendFuture<()>) {
    tokio::spawn(future);
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
