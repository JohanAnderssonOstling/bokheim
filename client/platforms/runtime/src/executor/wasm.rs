use super::startup::startup_ready;
use super::task::{owned_task, BackendTask};
use super::{BackendExecutorError, BackendFuture, BackendOutput, BackendSend, BackendTimeout};
use std::cell::Cell;
use std::future::Future;
use std::sync::OnceLock;
use std::time::Duration;
use wasm_bindgen::{closure::Closure, JsCast};

#[derive(Clone, Default)]
pub struct BackendExecutor;

impl BackendExecutor {
    pub fn new() -> Result<Self, BackendExecutorError> {
        Ok(Self)
    }

    /// Browser I/O already runs in the caller's context.
    pub async fn with_io_context<T>(&self, future: impl Future<Output = T>) -> T {
        future.await
    }

    pub fn spawn<T: BackendOutput>(&self, future: impl BackendFuture<T>) -> BackendTask<T> {
        let (task, runner) = owned_task(future);
        wasm_bindgen_futures::spawn_local(async move {
            startup_ready().await;
            runner.await
        });
        task
    }

    /// Storage stays on its OPFS worker and yields between synchronous batches.
    pub fn dispatch_storage(&self, operation: impl FnOnce() + Send + 'static) -> Result<(), BackendExecutorError> {
        STORAGE.get().ok_or_else(|| BackendExecutorError("storage executor is not initialized".into()))?.notify(move |()| async move {
            if sleep(Duration::ZERO).await.is_ok() {
                operation();
            }
        });
        Ok(())
    }

    pub fn storage_cleanup(&self, operation: impl FnOnce() + Send + 'static) {
        storage_cleanup(operation);
    }

    pub fn spawn_detached(&self, future: impl BackendFuture<()>) {
        wasm_bindgen_futures::spawn_local(async move {
            startup_ready().await;
            future.await
        });
    }
}

static STORAGE: OnceLock<super::ServiceHandle<()>> = OnceLock::new();
thread_local! {
    static ON_STORAGE: Cell<bool> = const { Cell::new(false) };
}

/// Install on the dedicated OPFS worker. Library operations keep running on
/// their caller; only file, database and other synchronous resource work hops here.
pub fn initialize_storage_executor() -> Result<(), BackendExecutorError> {
    STORAGE.set(super::local_service_on_current_thread((), ())).map_err(|_| BackendExecutorError("storage executor is already initialized".into()))?;
    ON_STORAGE.set(true);
    Ok(())
}

pub fn on_storage_thread() -> bool {
    ON_STORAGE.get()
}

pub async fn storage_call<T: Send + 'static, F: Future<Output = T> + 'static>(operation: impl FnOnce() -> F + Send + 'static) -> Result<T, BackendExecutorError> {
    if on_storage_thread() {
        return Ok(operation().await);
    }
    STORAGE.get().ok_or_else(|| BackendExecutorError("storage executor is not initialized".into()))?.call(move |()| operation()).await
}

pub fn storage_cleanup(operation: impl FnOnce() + Send + 'static) {
    if on_storage_thread() {
        operation();
    } else if let Some(storage) = STORAGE.get() {
        storage.notify(move |()| async move { operation() });
    }
}

pub async fn run_blocking<T: BackendOutput + Send>(operation: impl FnOnce() -> T + BackendSend + Send + 'static) -> Result<T, BackendExecutorError> {
    storage_call(move || async move { operation() }).await
}

pub fn spawn_detached(future: impl BackendFuture<()>) {
    wasm_bindgen_futures::spawn_local(async move {
        startup_ready().await;
        future.await
    });
}

struct Timer {
    scope: web_sys::WorkerGlobalScope,
    id: i32,
    _callback: Closure<dyn FnMut()>,
}
impl Timer {
    fn new(duration: Duration, callback: Closure<dyn FnMut()>) -> Result<Self, BackendExecutorError> {
        let scope = js_sys::global().unchecked_into::<web_sys::WorkerGlobalScope>();
        let id = scope
            .set_timeout_with_callback_and_timeout_and_arguments_0(callback.as_ref().unchecked_ref(), duration.as_millis().min(i32::MAX as u128) as i32)
            .map_err(|error| BackendExecutorError(format!("could not register browser timer: {error:?}")))?;
        Ok(Self { scope, id, _callback: callback })
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        self.scope.clear_timeout_with_handle(self.id);
    }
}

pub async fn sleep(duration: Duration) -> Result<(), BackendExecutorError> {
    let (reply, elapsed) = futures_channel::oneshot::channel();
    let _timer = Timer::new(
        duration,
        // This one-shot callback only settles its owned channel; a panic
        // cannot leave a reusable callback with partially updated state.
        Closure::once_assert_unwind_safe(move || {
            let _ = reply.send(());
        }),
    )?;
    elapsed.await.map_err(|_| BackendExecutorError("browser timer stopped before completing".into()))
}

pub async fn timeout<T>(duration: Duration, future: impl Future<Output = T>) -> Result<T, BackendTimeout> {
    tokio::select! {
        biased;
        output = future => Ok(output),
        elapsed = sleep(duration) => {
            if let Err(error) = elapsed {
                log::warn!("deadline could not be scheduled: {error}");
            }
            Err(BackendTimeout)
        },
    }
}

/// Works in both window and worker contexts, with a single-worker fallback.
pub fn available_parallelism() -> usize {
    js_sys::Reflect::get(&js_sys::global(), &"navigator".into())
        .ok()
        .and_then(|navigator| js_sys::Reflect::get(&navigator, &"hardwareConcurrency".into()).ok())
        .and_then(|value| value.as_f64())
        .filter(|value| value.is_finite() && *value >= 1.0)
        .map(|value| value as usize)
        .unwrap_or(1)
}
