//! Wi-Fi and Bluetooth share power rails on several Kobo models.
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::sync::atomic::{AtomicUsize, Ordering};

static RADIO: Mutex<()> = Mutex::new(());
static SLEEP_REQUESTS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn lock() -> MutexGuard<'static, ()> {
    RADIO.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Announce sleep before waiting for the radio worker/lock. Bluetooth commands
/// can then stop promptly instead of consuming the sleep preparation deadline.
pub(crate) struct SleepRequest;

impl SleepRequest {
    pub(crate) fn new() -> Self {
        SLEEP_REQUESTS.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for SleepRequest {
    fn drop(&mut self) {
        SLEEP_REQUESTS.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) fn sleep_requested() -> bool {
    SLEEP_REQUESTS.load(Ordering::Acquire) != 0
}
