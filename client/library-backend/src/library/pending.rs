//! Owned request work. Admission never waits in the command loop.
//!
//! Cancellation policy belongs to the workflow: accepted writes run
//! until completion or library retirement; disposable reads stop with their
//! caller. Directory imports finish the current file and inspect caller
//! cancellation between files. No database transaction may span an await.

use std::panic::AssertUnwindSafe;
use crate::BackendError;
use futures_util::{future::LocalBoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};
use std::{future::Future, rc::Rc};

#[derive(Clone, Copy)]
pub(crate) enum OperationKind {
    Read,
    Import,
}

/// A reply is resolved even if admission fails, a future panics, or its owner
/// is dropped before the future is first polled.
pub(crate) struct PendingReply<T> {
    sender: async_channel::Sender<Result<T, BackendError>>,
    finished: bool,
}

impl<T> PendingReply<T> {
    pub fn new(sender: async_channel::Sender<Result<T, BackendError>>) -> Self {
        Self { sender, finished: false }
    }
    #[cfg(target_arch = "wasm32")]
    pub fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }
    pub fn sender(&self) -> async_channel::Sender<Result<T, BackendError>> {
        self.sender.clone()
    }
    pub fn finish(mut self, result: Result<T, BackendError>) {
        self.finished = true;
        let _ = self.sender.try_send(result);
    }
}

impl<T> Drop for PendingReply<T> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.sender.try_send(Err(BackendError::message("library operation stopped before completing")));
        }
    }
}

pub(crate) struct PendingOperations<C> {
    tasks: FuturesUnordered<LocalBoxFuture<'static, C>>,
    reads: Rc<tokio::sync::Semaphore>,
    imports: Rc<tokio::sync::Semaphore>,
}

impl<C: 'static> Default for PendingOperations<C> {
    fn default() -> Self {
        Self { tasks: FuturesUnordered::new(), reads: Rc::new(tokio::sync::Semaphore::new(8)), imports: Rc::new(tokio::sync::Semaphore::new(1)) }
    }
}

impl<C: 'static> PendingOperations<C> {
    const MAX_PENDING: usize = 128;

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }
    pub async fn next(&mut self) -> Option<C> {
        self.tasks.next().await
    }

    pub fn submit<R: 'static, T: 'static>(
        &mut self, kind: OperationKind, reply: PendingReply<R>, operation: impl Future<Output = Result<T, BackendError>> + 'static, complete: impl FnOnce(PendingReply<R>, Result<T, BackendError>) -> C + 'static,
    ) -> bool {
        if self.tasks.len() >= Self::MAX_PENDING {
            reply.finish(Err(BackendError::message("too many pending library operations; retry later")));
            return false;
        }
        let permits = match kind {
            OperationKind::Read => self.reads.clone(),
            OperationKind::Import => self.imports.clone(),
        };
        let abandoned = reply.sender();
        self.tasks.push(Box::pin(async move {
            let run = async move {
                let _permit = permits.acquire().await.map_err(BackendError::operation)?;
                operation.await
            };
            let run = AssertUnwindSafe(run).catch_unwind();
            let result = if matches!(kind, OperationKind::Read) {
                tokio::select! {
                    biased;
                    _ = abandoned.closed() => Err(BackendError::message("library request cancelled")),
                    result = run => result.unwrap_or_else(|_| Err(BackendError::message("library operation panicked"))),
                }
            } else {
                run.await.unwrap_or_else(|_| Err(BackendError::message("library operation panicked")))
            };
            complete(reply, result)
        }));
        true
    }
}

/// One background lane: repeated wakes coalesce without replacing active work.
/// Channel closure alone retains queued messages while a sender survives.
/// Drain on every exit path, including panic and cancellation.
pub(crate) struct Inbox<T>(pub async_channel::Receiver<T>);
impl<T> Drop for Inbox<T> {
    fn drop(&mut self) {
        self.0.close();
        while self.0.try_recv().is_ok() {}
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn reply() -> (PendingReply<()>, async_channel::Receiver<Result<(), BackendError>>) {
        let (sender, receiver) = async_channel::bounded(1);
        (PendingReply::new(sender), receiver)
    }
    fn finish(reply: PendingReply<()>, result: Result<(), BackendError>) {
        reply.finish(result);
    }


    #[test]
    fn inbox_unwind_releases_queued_replies_even_with_a_retained_sender() {
        let (commands, receiver) = async_channel::unbounded();
        let (pending, response) = reply();
        commands.try_send(pending).unwrap_or_else(|_| panic!("inbox open"));
        let failed = std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _inbox = Inbox(receiver);
            panic!("actor failed");
        }));
        assert!(failed.is_err());
        assert!(commands.is_closed());
        assert!(response.try_recv().unwrap().is_err());
    }
}
