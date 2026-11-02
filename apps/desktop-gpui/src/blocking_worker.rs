//! Routes short desktop writes through the application's ordered storage queue.

use client_platform_runtime::storage_queue;

#[derive(Clone)]
pub(crate) struct BlockingWorker;

impl BlockingWorker {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn dispatch<T>(&self, work: impl FnOnce() -> T + Send + 'static) -> async_channel::Receiver<T>
    where
        T: Send + 'static,
    {
        let (result_tx, result_rx) = async_channel::bounded(1);
        let queued = storage_queue::dispatch_persistence(move || {
            if result_tx.is_closed() {
                return;
            }
            let result = work();
            let _ = result_tx.send_blocking(result);
        });
        if let Err(error) = queued {
            log::error!("could not queue session write: {error}");
        }
        result_rx
    }
}
