//! Typed calls to an event-loop owner. Futures are created on their owner;
//! only Rust arguments and results cross the thread boundary.
use super::BackendExecutorError;
use futures_util::{FutureExt, StreamExt, future::LocalBoxFuture, stream::FuturesUnordered};
use std::future::Future;
use std::panic::AssertUnwindSafe;
use tokio::sync::{mpsc, oneshot};

type Job<S> = Box<dyn FnOnce(S) -> LocalBoxFuture<'static, ()> + Send>;

pub struct ServiceHandle<S> {
    submit: mpsc::UnboundedSender<Job<S>>,
}

impl<S> Clone for ServiceHandle<S> {
    fn clone(&self) -> Self {
        Self { submit: self.submit.clone() }
    }
}

impl<S: Clone + 'static> ServiceHandle<S> {
    // Keep type erasure at the queue boundary. The operation creates its
    // future on the owner thread, so the future itself need not be Send.
    pub fn notify<F: Future<Output = ()> + 'static>(&self, operation: impl FnOnce(S) -> F + Send + 'static) {
        let _ = self.submit.send(Box::new(move |service| {
            Box::pin(async move {
                if AssertUnwindSafe(async move { operation(service).await }).catch_unwind().await.is_err() {
                    log::error!("service notification panicked");
                }
            })
        }));
    }

    pub async fn call<T: Send + 'static, F: Future<Output = T> + 'static>(&self, operation: impl FnOnce(S) -> F + Send + 'static) -> Result<T, BackendExecutorError> {
        let (reply, result) = oneshot::channel();
        self.submit
            .send(Box::new(move |service| {
                Box::pin(async move {
                    let outcome = AssertUnwindSafe(async move { operation(service).await }).catch_unwind().await;
                    let _ = reply.send(outcome.map_err(|_| BackendExecutorError("service operation panicked".into())));
                })
            }))
            .map_err(|_| BackendExecutorError("service is closed".into()))?;
        result.await.map_err(|_| BackendExecutorError("service stopped before replying".into()))?
    }
}

async fn serve<S: Clone + 'static, L: 'static>(service: S, lifetime: L, mut incoming: mpsc::UnboundedReceiver<Job<S>>) {
    let _lifetime = lifetime;
    let mut tasks = FuturesUnordered::new();
    loop {
        tokio::select! {
            job = incoming.recv() => match job {
                Some(job) => tasks.push(job(service.clone())),
                None => break,
            },
            Some(()) = tasks.next(), if !tasks.is_empty() => {},
        }
    }
    while tasks.next().await.is_some() {}
}

/// Starts an owner on the current local executor. Its resources never leave
/// this thread; only the returned handle can cross threads.
pub fn local_service_on_current_thread<S: Clone + 'static, L: 'static>(service: S, lifetime: L) -> ServiceHandle<S> {
    let (submit, incoming) = mpsc::unbounded_channel();
    #[cfg(not(target_arch = "wasm32"))]
    tokio::task::spawn_local(serve(service, lifetime, incoming));
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(serve(service, lifetime, incoming));
    ServiceHandle { submit }
}
