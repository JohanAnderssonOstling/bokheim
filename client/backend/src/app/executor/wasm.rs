use super::{BackendBlockingThunk, BackendExecutorError, BackendFuture, BackendOutput};
use futures_channel::oneshot;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

#[derive(Clone, Default)]
pub struct BackendExecutor;

pub struct BackendTask<T> {
    receiver: oneshot::Receiver<T>,
}

impl<T> Future for BackendTask<T> {
    type Output = Result<T, BackendExecutorError>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().receiver).poll(context).map(|result| result.map_err(|_| BackendExecutorError("backend task was cancelled".to_owned())))
    }
}

impl BackendExecutor {
    pub fn new() -> Result<Self, BackendExecutorError> {
        Ok(Self)
    }

    pub fn spawn<T: BackendOutput>(&self, future: impl BackendFuture<T>) -> BackendTask<T> {
        let (sender, receiver) = oneshot::channel();
        wasm_bindgen_futures::spawn_local(async move {
            let _ = sender.send(future.await);
        });
        BackendTask { receiver }
    }

    pub fn spawn_detached(&self, future: impl BackendFuture<()>) {
        wasm_bindgen_futures::spawn_local(future);
    }
}

pub async fn run_blocking<T, F>(operation: F) -> Result<T, BackendExecutorError>
where
    T: BackendOutput,
    F: BackendBlockingThunk<T>,
{
    Ok(operation())
}

pub(crate) fn spawn_detached(future: impl BackendFuture<()>) {
    wasm_bindgen_futures::spawn_local(future);
}
