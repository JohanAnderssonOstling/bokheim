use super::task::{owned_task, BackendTask};
use super::{BackendExecutorError, BackendFuture, BackendOutput, BackendSend, BackendTimeout};
use std::future::Future;
use std::time::Duration;
use wasm_bindgen::{closure::Closure, JsCast};

#[derive(Clone, Default)]
pub struct BackendExecutor;

impl BackendExecutor {
    pub fn new() -> Result<Self, BackendExecutorError> {
        Ok(Self)
    }

    pub fn spawn<T: BackendOutput>(&self, future: impl BackendFuture<T>) -> BackendTask<T> {
        let (task, runner) = owned_task(future);
        wasm_bindgen_futures::spawn_local(runner);
        task
    }

    pub fn spawn_detached(&self, future: impl BackendFuture<()>) {
        wasm_bindgen_futures::spawn_local(future);
    }
}

pub async fn run_blocking<T: BackendOutput>(operation: impl FnOnce() -> T + BackendSend + 'static) -> Result<T, BackendExecutorError> {
    Ok(operation())
}

pub fn spawn_detached(future: impl BackendFuture<()>) {
    wasm_bindgen_futures::spawn_local(future);
}

struct Timer {
    scope: web_sys::WorkerGlobalScope,
    id: i32,
    _callback: Closure<dyn FnMut()>,
}
impl Timer {
    fn new(duration: Duration, callback: Closure<dyn FnMut()>) -> Self {
        let scope = js_sys::global().unchecked_into::<web_sys::WorkerGlobalScope>();
        let id = scope.set_timeout_with_callback_and_timeout_and_arguments_0(callback.as_ref().unchecked_ref(), duration.as_millis().min(i32::MAX as u128) as i32).expect("backend worker must support timers");
        Self { scope, id, _callback: callback }
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        self.scope.clear_timeout_with_handle(self.id);
    }
}

pub async fn sleep(duration: Duration) {
    let (reply, elapsed) = futures_channel::oneshot::channel();
    let _timer = Timer::new(duration, Closure::once(move || { let _ = reply.send(()); }));
    let _ = elapsed.await;
}

pub async fn timeout<T>(duration: Duration, future: impl Future<Output = T>) -> Result<T, BackendTimeout> {
    tokio::select! {
        biased;
        output = future => Ok(output),
        _ = sleep(duration) => Err(BackendTimeout),
    }
}

/// Exercises the production timer's ownership in the WASM contract tests.
#[cfg(feature = "web-runtime-tests")]
pub async fn timer_lifecycle_contract() -> bool {
    use std::rc::Rc;
    let resource = Rc::new(());
    for _ in 0..32 {
        let captured = resource.clone();
        let timer = Timer::new(Duration::from_secs(60), Closure::once(move || drop(captured)));
        if Rc::strong_count(&resource) != 2 { return false; }
        drop(timer);
        if Rc::strong_count(&resource) != 1 { return false; }
    }
    let captured = resource.clone();
    let (reply, elapsed) = futures_channel::oneshot::channel();
    let timer = Timer::new(Duration::from_millis(1), Closure::once(move || {
        drop(captured);
        let _ = reply.send(());
    }));
    if elapsed.await.is_err() { return false; }
    drop(timer);
    if Rc::strong_count(&resource) != 1 { return false; }
    let mut cancelled = Box::pin(sleep(Duration::from_secs(60)));
    if !futures_util::poll!(cancelled.as_mut()).is_pending() { return false; }
    drop(cancelled);
    let captured = resource.clone();
    let task = BackendExecutor::default().spawn(async move {
        let _resource = captured;
        sleep(Duration::from_secs(300)).await;
    });
    sleep(Duration::ZERO).await;
    if Rc::strong_count(&resource) != 2 { return false; }
    drop(task);
    sleep(Duration::ZERO).await;
    if Rc::strong_count(&resource) != 1 { return false; }
    timeout(Duration::from_secs(60), async { 7 }).await == Ok(7)
        && timeout(Duration::from_millis(1), std::future::pending::<()>()).await == Err(BackendTimeout)
}

/// Works in both window and worker contexts, with a single-worker fallback.
pub fn available_parallelism() -> usize {
    js_sys::Reflect::get(&js_sys::global(), &"navigator".into()).ok()
        .and_then(|navigator| js_sys::Reflect::get(&navigator, &"hardwareConcurrency".into()).ok())
        .and_then(|value| value.as_f64())
        .filter(|value| value.is_finite() && *value >= 1.0)
        .map(|value| value as usize).unwrap_or(1)
}
