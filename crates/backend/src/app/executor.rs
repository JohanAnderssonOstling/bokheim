use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

/// The single async-I/O execution boundary owned by an application host.
///
/// Standalone hosts construct one executor. Hosts that already own Tokio, such
/// as Tauri, inject its handle instead so the backend does not create a second
/// runtime.
#[derive(Clone)]
pub struct BackendExecutor {
    inner: Arc<BackendExecutorInner>,
}

struct BackendExecutorInner {
    handle: tokio::runtime::Handle,
    owned_runtime: Option<OwnedRuntime>,
}

struct OwnedRuntime(Mutex<Option<tokio::runtime::Runtime>>);

impl Drop for OwnedRuntime {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.get_mut().ok().and_then(Option::take) {
            runtime.shutdown_background();
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendExecutorError(String);

impl fmt::Display for BackendExecutorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for BackendExecutorError {}

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
        Ok(Self { inner: Arc::new(BackendExecutorInner { handle, owned_runtime: Some(OwnedRuntime(Mutex::new(Some(runtime)))) }) })
    }

    pub fn from_tokio_handle(handle: tokio::runtime::Handle) -> Result<Self, BackendExecutorError> {
        if matches!(handle.runtime_flavor(), tokio::runtime::RuntimeFlavor::CurrentThread) {
            return Err(BackendExecutorError("the backend requires a continuously driven multi-thread Tokio runtime".to_owned()));
        }
        Ok(Self { inner: Arc::new(BackendExecutorInner { handle, owned_runtime: None }) })
    }

    pub fn from_current_tokio() -> Result<Self, BackendExecutorError> {
        let handle = tokio::runtime::Handle::try_current().map_err(|error| BackendExecutorError(format!("no Tokio runtime is active on this thread: {error}")))?;
        Self::from_tokio_handle(handle)
    }

    pub fn spawn<T>(&self, future: impl Future<Output = T> + Send + 'static) -> BackendTask<T>
    where
        T: Send + 'static,
    {
        BackendTask { task: self.inner.handle.spawn(future) }
    }

    pub fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        // Only synchronous host/worker threads use this compatibility bridge.
        // Async UI integrations use `spawn` and await the returned BackendTask.
        self.inner.handle.block_on(future)
    }

    pub fn owns_runtime(&self) -> bool {
        self.inner.owned_runtime.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    fn assert_send<T: Send>() {}

    #[test]
    fn owned_executor_reuses_one_runtime_for_spawn_and_block_on() {
        let executor = BackendExecutor::new().unwrap();
        assert!(executor.owns_runtime());
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
    fn injected_executor_reuses_the_host_runtime() {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let executor = BackendExecutor::from_tokio_handle(runtime.handle().clone()).unwrap();
        assert!(!executor.owns_runtime());
        let task = executor.spawn(async {
            tokio::time::sleep(Duration::from_millis(1)).await;
            42_u8
        });
        assert_eq!(runtime.block_on(task).unwrap(), 42);
    }

    #[test]
    fn current_executor_reuses_the_entered_host_runtime() {
        let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let executor = runtime.block_on(async { BackendExecutor::from_current_tokio().unwrap() });
        assert!(!executor.owns_runtime());
        let task = executor.spawn(async { 42_u8 });
        assert_eq!(runtime.block_on(task).unwrap(), 42);
    }

    #[test]
    fn borrowed_current_thread_runtime_is_rejected() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        assert!(BackendExecutor::from_tokio_handle(runtime.handle().clone()).is_err());
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
