//! RAII cleanup and cancellation scopes for async work.
use std::future::Future;

/// Capture before constructing an async block so even an unpolled future cleans up.
pub struct OnDrop<F: FnOnce()>(Option<F>);
impl<F: FnOnce()> OnDrop<F> {
    pub fn new(cleanup: F) -> Self {
        Self(Some(cleanup))
    }
}
impl<F: FnOnce()> Drop for OnDrop<F> {
    fn drop(&mut self) {
        if let Some(cleanup) = self.0.take() {
            cleanup();
        }
    }
}

/// Cancellation wins ties and drops the operation before returning to its owner.
pub async fn until_cancelled<T>(operation: impl Future<Output = T>, cancelled: impl Future) -> Option<T> {
    futures_util::pin_mut!(operation, cancelled);
    match futures_util::future::select(cancelled, operation).await {
        futures_util::future::Either::Left(_) => None,
        futures_util::future::Either::Right((result, _)) => Some(result),
    }
}
