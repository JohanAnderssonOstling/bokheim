//! Correlation and disconnect draining shared by worker transports.

use std::future::Future;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
};

pub struct Pending<T> {
    next: Cell<u64>,
    entries: RefCell<HashMap<u64, T>>,
}
impl<T> Default for Pending<T> {
    fn default() -> Self {
        Self { next: Cell::new(0), entries: RefCell::new(HashMap::new()) }
    }
}
impl<T> Pending<T> {
    pub fn next_id(&self) -> u64 {
        let id = self.next.get().checked_add(1).expect("browser request IDs exhausted");
        self.next.set(id);
        id
    }
    pub fn insert(&self, id: u64, value: T) {
        self.entries.borrow_mut().insert(id, value);
    }
    pub fn take(&self, id: u64) -> Option<T> {
        self.entries.borrow_mut().remove(&id)
    }
    /// Release the map borrow before callbacks: settlement may remove itself or
    /// submit another request. New requests belong to the next connection.
    pub fn fail_all(&self, mut fail: impl FnMut(T)) {
        let entries = std::mem::take(&mut *self.entries.borrow_mut());
        for entry in entries.into_values() {
            fail(entry);
        }
    }
}

/// Removing a request on future drop prevents cancelled calls retaining replies.
pub struct Registration<'a, T> {
    pending: &'a Pending<T>,
    id: u64,
}
impl<T> Pending<T> {
    pub fn track(&self, id: u64, value: T) -> Registration<'_, T> {
        self.insert(id, value);
        Registration { pending: self, id }
    }
}
impl<T> Drop for Registration<'_, T> {
    fn drop(&mut self) {
        self.pending.take(self.id);
    }
}


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
