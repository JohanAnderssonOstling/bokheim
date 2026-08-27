//! Asynchronous library persistence coordinated by the reader.
//!
//! The backend owns any platform-specific blocking work. This module only
//! folds loaded data back into its entity, serializes optimistic mutations,
//! and debounces position updates.

use std::rc::Rc;
use std::time::Duration;
use std::{future::Future, pin::Pin};

use bokheim_backend::app::executor::{BackendBlockingOperation, BackendOutput};
use bokheim_backend::{BackendError, LibraryBackend};
use gpui::{App, Context};

pub(crate) type PersistenceFuture<T> = Pin<Box<dyn Future<Output = Result<T, String>> + 'static>>;
type Mutation = Box<dyn FnOnce(&LibraryBackend) -> PersistenceFuture<()> + 'static>;

pub(crate) fn run<T, F>(library: &LibraryBackend, operation: F) -> PersistenceFuture<T>
where
    T: BackendOutput,
    F: BackendBlockingOperation<LibraryBackend, Result<T, BackendError>>,
{
    let task = library.spawn_blocking(operation);
    Box::pin(async move { task.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
}

/// Awaits `op` against the library, then applies `apply` to the view with its
/// result.
///
/// `what` names the write for log messages, phrased to follow "failed to" —
/// for example `"save annotation"`. If the operation fails, the failure is
/// logged and `apply` is not called.
pub(crate) fn write<V, T>(view: &Context<V>, library: Rc<LibraryBackend>, what: &'static str, op: impl FnOnce(&LibraryBackend) -> PersistenceFuture<T> + 'static, apply: impl FnOnce(&mut V, &mut Context<V>, T) + 'static)
where
    V: 'static,
    T: 'static,
{
    view.spawn(async move |this, cx| {
        let result = op(library.as_ref()).await;
        let _ = this.update(cx, |this, cx| match result {
            Ok(value) => apply(this, cx, value),
            Err(error) => log::error!("failed to {what}: {error}"),
        });
    })
    .detach();
}

/// Serializes durable mutations independently of UI updates.
///
/// Reader state is changed optimistically on the GPUI thread. This queue then
/// preserves the same order in storage.
pub(crate) struct MutationWriter {
    mutations: async_channel::Sender<(&'static str, Mutation)>,
}

impl MutationWriter {
    pub(crate) fn new(cx: &App, library: Rc<LibraryBackend>) -> Self {
        let (mutations, incoming) = async_channel::unbounded::<(&'static str, Mutation)>();
        cx.spawn(async move |_cx| {
            while let Ok((what, mutation)) = incoming.recv().await {
                if let Err(error) = mutation(library.as_ref()).await {
                    log::error!("failed to {what}: {error}");
                }
            }
        })
        .detach();
        Self { mutations }
    }

    pub(crate) fn record(&self, what: &'static str, mutation: impl FnOnce(&LibraryBackend) -> PersistenceFuture<()> + 'static) {
        if self.mutations.try_send((what, Box::new(mutation))).is_err() {
            log::error!("failed to queue {what}");
        }
    }
}

/// How long a position update waits for a quieter one to supersede it.
const POSITION_DEBOUNCE: Duration = Duration::from_millis(250);

/// Coalesces a stream of reading-position updates into occasional library
/// writes.
///
/// Page turns arrive far faster than the database should be written. Each
/// update waits out [`POSITION_DEBOUNCE`]; anything that queued up meanwhile is
/// drained and only the newest position is persisted.
pub(crate) struct PositionWriter<P> {
    updates: async_channel::Sender<P>,
}

impl<P: 'static> PositionWriter<P> {
    /// Starts the debouncing task. `persist` performs the actual library
    /// write for the newest position; `what` names it for log messages.
    pub(crate) fn new(cx: &App, what: &'static str, persist: impl Fn(&LibraryBackend, &P) -> PersistenceFuture<()> + 'static, library: Rc<LibraryBackend>) -> Self {
        let (updates, incoming) = async_channel::unbounded::<P>();
        let executor = cx.background_executor().clone();
        let persist = Rc::new(persist);
        cx.spawn(async move |_cx| {
            while let Ok(mut position) = incoming.recv().await {
                executor.timer(POSITION_DEBOUNCE).await;
                while let Ok(newer) = incoming.try_recv() {
                    position = newer;
                }
                if let Err(error) = persist(library.as_ref(), &position).await {
                    log::error!("failed to {what}: {error}");
                }
            }
        })
        .detach();
        Self { updates }
    }

    /// Records a new position. Dropping it because the task has stopped is
    /// logged once per update and is not otherwise fatal.
    pub(crate) fn record(&self, position: P, what: &'static str) {
        if self.updates.try_send(position).is_err() {
            log::warn!("{what} persistence task stopped");
        }
    }
}
