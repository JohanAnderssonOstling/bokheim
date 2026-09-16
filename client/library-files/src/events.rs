//! Notifications emitted by filesystem discovery.
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanEvent {
    ScanStarted,
    ScanComplete,
    TransferStatusChanged,
}

/// The host decides how scanner notifications reach its event loop.
/// The callback runs on scanner threads and should enqueue events without blocking.
#[derive(Clone)]
pub struct ScanEventSink(Arc<dyn Fn(ScanEvent) + Send + Sync>);
impl ScanEventSink {
    pub fn new(emit: impl Fn(ScanEvent) + Send + Sync + 'static) -> Self {
        Self(Arc::new(emit))
    }
    pub(crate) fn emit(&self, event: ScanEvent) {
        (self.0)(event);
    }
}

#[cfg(test)]
pub(crate) fn test_channel() -> (ScanEventSink, async_channel::Receiver<ScanEvent>) {
    let (sender, receiver) = async_channel::unbounded();
    (
        ScanEventSink::new(move |event| {
            let _ = sender.try_send(event);
        }),
        receiver,
    )
}
