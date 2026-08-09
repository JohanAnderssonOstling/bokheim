//! Library writes made off the UI thread.
//!
//! Every reader persists the same way: hand the work to the persistence
//! worker, wait for it, then fold the result back into the view. The failure
//! policy is uniform — log and carry on, never panic and never block the
//! reader — so it lives here rather than at each call site.

use std::sync::Arc;
use std::time::Duration;

use epub_svelte_lib::LibraryBackend;
use gpui::{App, Context};

use crate::settings::ReaderServices;

/// Runs `op` against the library on the persistence worker, then applies
/// `apply` to the view with its result.
///
/// `what` names the write for log messages, phrased to follow "failed to" —
/// for example `"save annotation"`. If the write fails, or the worker has
/// stopped, the failure is logged and `apply` is not called.
pub(crate) fn write<V, T>(view: &Context<V>, library: LibraryBackend, what: &'static str, op: impl FnOnce(&LibraryBackend) -> Result<T, String> + Send + 'static, apply: impl FnOnce(&mut V, &mut Context<V>, T) + 'static)
where
    V: 'static,
    T: Send + 'static,
{
    let worker = ReaderServices::get(view).persistence.clone();
    view.spawn(async move |this, cx| {
        let result = worker.dispatch(move || op(&library)).recv().await;
        let _ = this.update(cx, |this, cx| match result {
            Ok(Ok(value)) => apply(this, cx, value),
            Ok(Err(error)) => log::error!("failed to {what}: {error}"),
            Err(_) => log::error!("{what} worker stopped"),
        });
    })
    .detach();
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

impl<P: Send + 'static> PositionWriter<P> {
    /// Starts the debouncing worker. `persist` performs the actual library
    /// write for the newest position; `what` names it for log messages.
    pub(crate) fn new(cx: &App, what: &'static str, persist: impl Fn(&LibraryBackend, &P) -> Result<(), String> + Send + Sync + 'static, library: LibraryBackend) -> Self {
        let (updates, incoming) = async_channel::unbounded::<P>();
        let worker = ReaderServices::get(cx).persistence.clone();
        let executor = cx.background_executor().clone();
        let persist = Arc::new(persist);
        cx.background_executor()
            .spawn(async move {
                while let Ok(mut position) = incoming.recv().await {
                    executor.timer(POSITION_DEBOUNCE).await;
                    while let Ok(newer) = incoming.try_recv() {
                        position = newer;
                    }
                    let library = library.clone();
                    let persist = persist.clone();
                    let Ok(result) = worker.dispatch(move || persist(&library, &position)).recv().await else {
                        break;
                    };
                    if let Err(error) = result {
                        log::error!("failed to {what}: {error}");
                    }
                }
            })
            .detach();
        Self { updates }
    }

    /// Records a new position. Dropping it because the worker has stopped is
    /// logged once per update and is not otherwise fatal.
    pub(crate) fn record(&self, position: P, what: &'static str) {
        if self.updates.try_send(position).is_err() {
            log::warn!("{what} worker stopped");
        }
    }
}
