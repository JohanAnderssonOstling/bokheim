//! Owned request work. Admission never waits in the command loop.
//!
//! Cancellation policy belongs to the workflow: accepted writes run
//! until completion or library retirement; disposable reads stop with their
//! caller. Directory imports finish the current file and inspect caller
//! cancellation between files. No database transaction may span an await.

use super::handle::OperationCompletion;
use super::transfer::TransferResource;
use crate::BackendError;
#[cfg(not(target_arch = "wasm32"))]
use crate::{library::ResolvedBook, ImportFailure};
use client_runtime::reply::Reply;
use futures_util::{future::LocalBoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};
use library_database::transfer::ImportedBook;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;

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

/// Typed work retains its reply guard while the scheduler catches panics.
/// Only the operation future is erased; results keep their concrete types.
pub(crate) struct PendingWork<R, T = R> {
    reply: Option<PendingReply<R>>,
    work: LocalBoxFuture<'static, Result<T, BackendError>>,
}

impl<R: 'static, T: 'static> PendingWork<R, T> {
    pub(crate) fn new(reply: PendingReply<R>, work: LocalBoxFuture<'static, Result<T, BackendError>>) -> Self {
        Self { reply: Some(reply), work }
    }

    async fn run(&mut self) -> (PendingReply<R>, Result<T, BackendError>) {
        let result = self.work.as_mut().await;
        (self.reply.take().expect("pending work owns its reply"), result)
    }

    fn fail(mut self, error: BackendError) -> (PendingReply<R>, Result<T, BackendError>) {
        (self.reply.take().expect("pending work owns its reply"), Err(error))
    }

    fn reject(mut self, error: BackendError) {
        self.reply.take().expect("pending work owns its reply").finish(Err(error));
    }

    fn sender(&self) -> async_channel::Sender<Result<R, BackendError>> {
        self.reply.as_ref().expect("pending work owns its reply").sender.clone()
    }
}

/// Each variant pairs an operation with the channel that accepts its result.
pub(crate) enum PendingTask {
    Reply(PendingWork<Reply>),
    #[cfg(not(target_arch = "wasm32"))]
    ResolvedBook(PendingWork<ResolvedBook>),
    ExportTransferBook(PendingWork<TransferResource>),
    ImportTransferBook(PendingWork<ImportedBook>),
    #[cfg(not(target_arch = "wasm32"))]
    ImportDirectory(PendingWork<Vec<ImportFailure>>),
    #[cfg(target_arch = "wasm32")]
    Book(PendingWork<(Reply, u64), super::book::Opened>),
}

/// An independent channel-close watcher; it never owns a reply guard.
/// Keeping this concrete avoids allocating another future for each read.
enum ReplyCancellation {
    Reply(async_channel::Sender<Result<Reply, BackendError>>),
    #[cfg(not(target_arch = "wasm32"))]
    ResolvedBook(async_channel::Sender<Result<ResolvedBook, BackendError>>),
    ExportTransferBook(async_channel::Sender<Result<TransferResource, BackendError>>),
    ImportTransferBook(async_channel::Sender<Result<ImportedBook, BackendError>>),
    #[cfg(not(target_arch = "wasm32"))]
    ImportDirectory(async_channel::Sender<Result<Vec<ImportFailure>, BackendError>>),
    #[cfg(target_arch = "wasm32")]
    Book(async_channel::Sender<Result<(Reply, u64), BackendError>>),
}

impl ReplyCancellation {
    async fn wait(self) {
        match self {
            Self::Reply(sender) => sender.closed().await,
            #[cfg(not(target_arch = "wasm32"))]
            Self::ResolvedBook(sender) => sender.closed().await,
            Self::ExportTransferBook(sender) => sender.closed().await,
            Self::ImportTransferBook(sender) => sender.closed().await,
            #[cfg(not(target_arch = "wasm32"))]
            Self::ImportDirectory(sender) => sender.closed().await,
            #[cfg(target_arch = "wasm32")]
            Self::Book(sender) => sender.closed().await,
        }
    }
}

impl PendingTask {
    async fn run(&mut self) -> OperationCompletion {
        match self {
            Self::Reply(work) => {
                let (reply, result) = work.run().await;
                OperationCompletion::Reply { reply, result }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Self::ResolvedBook(work) => {
                let (reply, result) = work.run().await;
                OperationCompletion::ResolvedBook { reply, result }
            }
            Self::ExportTransferBook(work) => {
                let (reply, result) = work.run().await;
                OperationCompletion::ExportTransferBook { reply, result }
            }
            Self::ImportTransferBook(work) => {
                let (reply, result) = work.run().await;
                OperationCompletion::ImportTransferBook { reply, result }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Self::ImportDirectory(work) => {
                let (reply, result) = work.run().await;
                OperationCompletion::ImportDirectory { reply, result }
            }
            #[cfg(target_arch = "wasm32")]
            Self::Book(work) => {
                let (reply, result) = work.run().await;
                OperationCompletion::Book { reply, result }
            }
        }
    }

    fn fail(self, error: BackendError) -> OperationCompletion {
        match self {
            Self::Reply(work) => {
                let (reply, result) = work.fail(error);
                OperationCompletion::Reply { reply, result }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Self::ResolvedBook(work) => {
                let (reply, result) = work.fail(error);
                OperationCompletion::ResolvedBook { reply, result }
            }
            Self::ExportTransferBook(work) => {
                let (reply, result) = work.fail(error);
                OperationCompletion::ExportTransferBook { reply, result }
            }
            Self::ImportTransferBook(work) => {
                let (reply, result) = work.fail(error);
                OperationCompletion::ImportTransferBook { reply, result }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Self::ImportDirectory(work) => {
                let (reply, result) = work.fail(error);
                OperationCompletion::ImportDirectory { reply, result }
            }
            #[cfg(target_arch = "wasm32")]
            Self::Book(work) => {
                let (reply, result) = work.fail(error);
                OperationCompletion::Book { reply, result }
            }
        }
    }

    fn reject(self, error: BackendError) {
        match self {
            Self::Reply(work) => work.reject(error),
            #[cfg(not(target_arch = "wasm32"))]
            Self::ResolvedBook(work) => work.reject(error),
            Self::ExportTransferBook(work) => work.reject(error),
            Self::ImportTransferBook(work) => work.reject(error),
            #[cfg(not(target_arch = "wasm32"))]
            Self::ImportDirectory(work) => work.reject(error),
            #[cfg(target_arch = "wasm32")]
            Self::Book(work) => work.reject(error),
        }
    }

    fn cancellation(&self) -> ReplyCancellation {
        match self {
            Self::Reply(work) => ReplyCancellation::Reply(work.sender()),
            #[cfg(not(target_arch = "wasm32"))]
            Self::ResolvedBook(work) => ReplyCancellation::ResolvedBook(work.sender()),
            Self::ExportTransferBook(work) => ReplyCancellation::ExportTransferBook(work.sender()),
            Self::ImportTransferBook(work) => ReplyCancellation::ImportTransferBook(work.sender()),
            #[cfg(not(target_arch = "wasm32"))]
            Self::ImportDirectory(work) => ReplyCancellation::ImportDirectory(work.sender()),
            #[cfg(target_arch = "wasm32")]
            Self::Book(work) => ReplyCancellation::Book(work.sender()),
        }
    }
}

pub(crate) struct PendingOperations {
    tasks: FuturesUnordered<LocalBoxFuture<'static, OperationCompletion>>,
    reads: Rc<tokio::sync::Semaphore>,
    imports: Rc<tokio::sync::Semaphore>,
}

impl Default for PendingOperations {
    fn default() -> Self {
        Self { tasks: FuturesUnordered::new(), reads: Rc::new(tokio::sync::Semaphore::new(8)), imports: Rc::new(tokio::sync::Semaphore::new(1)) }
    }
}

impl PendingOperations {
    const MAX_PENDING: usize = 128;

    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    pub async fn next(&mut self) -> Option<OperationCompletion> {
        self.tasks.next().await
    }

    pub fn submit(&mut self, kind: OperationKind, task: PendingTask) -> bool {
        if self.tasks.len() >= Self::MAX_PENDING {
            task.reject(BackendError::message("too many pending library operations; retry later"));
            return false;
        }
        let permits = match kind {
            OperationKind::Read => self.reads.clone(),
            OperationKind::Import => self.imports.clone(),
        };
        self.tasks.push(Box::pin(run_pending(kind, task, permits)));
        true
    }
}

/// One scheduling future for every operation and reply type. Keep the task
/// outside the panic boundary so failures still return a typed completion.
async fn run_pending(kind: OperationKind, mut task: PendingTask, permits: Rc<tokio::sync::Semaphore>) -> OperationCompletion {
    let cancellation = matches!(kind, OperationKind::Read).then(|| task.cancellation());
    let result = {
        let run = async {
            let _permit = permits.acquire().await.map_err(BackendError::operation)?;
            Ok(task.run().await)
        };
        let run = AssertUnwindSafe(run).catch_unwind().map(|result| result.unwrap_or_else(|_| Err(BackendError::message("library operation panicked"))));
        match cancellation {
            Some(cancelled) => tokio::select! {
                biased;
                _ = cancelled.wait() => Err(BackendError::message("library request cancelled")),
                result = run => result,
            },
            None => run.await,
        }
    };
    // The borrowing run future and its permit have dropped before failure
    // consumes the task and releases the operation's remaining resources.
    match result {
        Ok(completion) => completion,
        Err(error) => task.fail(error),
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
    use std::{
        cell::Cell,
        future::{pending, Future},
        task::{Context, Poll, Waker},
        time::Duration,
    };
    use tokio::time::timeout;

    fn reply() -> (PendingReply<Reply>, async_channel::Receiver<Result<Reply, BackendError>>) {
        let (sender, receiver) = async_channel::bounded(1);
        (PendingReply::new(sender), receiver)
    }
    fn finish(completion: OperationCompletion) {
        let OperationCompletion::Reply { reply, result } = completion else { panic!("expected a reply completion") };
        reply.finish(result);
    }

    struct Dropped(Rc<Cell<bool>>);

    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.set(true);
        }
    }

    #[tokio::test]
    async fn disconnected_reads_drop_unpolled_or_running_work_and_release_admission() {
        for poll_before_disconnect in [false, true] {
            let mut operations = PendingOperations::default();
            let permits = operations.reads.clone();
            let available = permits.available_permits();
            let started = Rc::new(Cell::new(false));
            let observed = started.clone();
            let dropped = Rc::new(Cell::new(false));
            let guard = Dropped(dropped.clone());
            let (reply, response) = reply();
            assert!(operations.submit(
                OperationKind::Read,
                PendingTask::Reply(PendingWork::new(
                    reply,
                    Box::pin(async move {
                        let _guard = guard;
                        observed.set(true);
                        pending::<Result<Reply, BackendError>>().await
                    })
                ))
            ));
            {
                let next = operations.next();
                tokio::pin!(next);
                if poll_before_disconnect {
                    assert!(matches!(next.as_mut().poll(&mut Context::from_waker(Waker::noop())), Poll::Pending));
                    assert!(started.get());
                    assert_eq!(permits.available_permits(), available - 1);
                }
                drop(response);
                finish(timeout(Duration::from_secs(1), next).await.unwrap().expect("read completion"));
            }
            assert_eq!(started.get(), poll_before_disconnect);
            assert!(dropped.get());
            assert_eq!(permits.available_permits(), available);
            assert!(operations.is_empty());
        }
    }

    #[tokio::test]
    async fn disconnected_imports_run_to_completion_and_release_admission() {
        let mut operations = PendingOperations::default();
        let permits = operations.imports.clone();
        let available = permits.available_permits();
        let started = Rc::new(Cell::new(false));
        let observed = started.clone();
        let completed = Rc::new(Cell::new(false));
        let completion = completed.clone();
        let (release, released) = async_channel::bounded(1);
        let (reply, response) = reply();
        assert!(operations.submit(
            OperationKind::Import,
            PendingTask::Reply(PendingWork::new(
                reply,
                Box::pin(async move {
                    observed.set(true);
                    released.recv().await.unwrap();
                    completion.set(true);
                    Ok(Reply::new(()))
                })
            ))
        ));
        drop(response);
        {
            let next = operations.next();
            tokio::pin!(next);
            assert!(matches!(next.as_mut().poll(&mut Context::from_waker(Waker::noop())), Poll::Pending));
            assert!(started.get());
            assert!(!completed.get());
            assert_eq!(permits.available_permits(), available - 1);
            release.send(()).await.unwrap();
            finish(timeout(Duration::from_secs(1), next).await.unwrap().expect("import completion"));
        }
        assert!(completed.get());
        assert_eq!(permits.available_permits(), available);
        assert!(operations.is_empty());
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
