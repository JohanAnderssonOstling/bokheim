//! Admission accounting follows owned requests through dispatch and cancellation.
use std::{cell::Cell, rc::Rc};
use tokio::sync::Notify;
#[derive(Clone)]
pub struct ByteBudget {
    used: Rc<Cell<usize>>,
    released: Rc<Notify>,
    limit: usize,
}
pub struct Reservation {
    used: Rc<Cell<usize>>,
    released: Rc<Notify>,
    bytes: usize,
}
impl ByteBudget {
    pub fn new(limit: usize) -> Self {
        Self { used: Rc::new(Cell::new(0)), released: Rc::new(Notify::new()), limit }
    }
    pub fn reserve(&self, bytes: usize) -> Option<Reservation> {
        let next = self.used.get().checked_add(bytes)?;
        if next > self.limit {
            return None;
        }
        self.used.set(next);
        Some(Reservation { used: self.used.clone(), released: self.released.clone(), bytes })
    }
    /// Wait for admitted work to release capacity; a request that cannot fit
    /// even in an empty budget still fails immediately.
    pub async fn reserve_when_available(&self, bytes: usize) -> Option<Reservation> {
        if bytes > self.limit {
            return None;
        }
        loop {
            let released = self.released.notified();
            tokio::pin!(released);
            // Register before checking capacity so a release cannot be missed.
            released.as_mut().enable();
            if let Some(reservation) = self.reserve(bytes) {
                return Some(reservation);
            }
            released.await;
        }
    }
    pub fn limit(&self) -> usize {
        self.limit
    }
    pub fn used(&self) -> usize {
        self.used.get()
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        self.used.set(self.used.get() - self.bytes);
        self.released.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{coordinator::CpuJobs, worker_lifecycle::Connection};
    #[test]
    fn budget_rejects_oversize_and_overflow_without_changing_usage() {
        let budget = ByteBudget::new(10);
        let first = budget.reserve(8).unwrap();
        assert!(budget.reserve(3).is_none());
        assert!(budget.reserve(usize::MAX).is_none());
        assert_eq!(budget.used(), 8);
        drop(first);
        assert_eq!(budget.used(), 0);
        assert!(budget.reserve(10).is_some());
    }
    #[test]
    fn repeated_startup_cancellation_failure_and_stale_reply_release_every_request() {
        let budget = ByteBudget::new(10);
        let mut jobs = CpuJobs::new(3);
        for cycle in 0..100 {
            let first = cycle * 3 + 1;
            let old = u64::from(first);
            let worker = Connection::new((), old, ());
            assert!(jobs.request(first, budget.reserve(4).unwrap()).is_ok());
            assert!(jobs.request(first + 1, budget.reserve(6).unwrap()).is_ok());
            drop(jobs.cancel_pending(first));
            assert_eq!(budget.used(), 6);
            drop(worker);
            drop(jobs.fail());
            assert_eq!(budget.used(), 0);
            let new = old + 1;
            let mut worker = Connection::new((), new, ());
            assert!(jobs.request(first + 2, budget.reserve(10).unwrap()).is_ok());
            worker.ready(old);
            assert!(!worker.accepts(old));
            assert!(worker.waiting(new));
            assert!(jobs.complete(first).is_err());
            worker.ready(new);
            jobs.ready();
            jobs.next();
            drop(jobs.complete(first + 2).unwrap());
            assert_eq!(budget.used(), 0);
        }
    }
}
