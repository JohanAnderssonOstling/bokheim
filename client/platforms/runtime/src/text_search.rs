//! Device-local admission for the rebuildable book-text index.
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::watch;

static ENABLED: AtomicBool = AtomicBool::new(false);
static CHANGES: OnceLock<watch::Sender<()>> = OnceLock::new();

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}
pub fn set_enabled(value: bool) {
    if ENABLED.swap(value, Ordering::AcqRel) != value {
        CHANGES.get_or_init(|| watch::channel(()).0).send_replace(());
    }
}
pub fn subscribe() -> watch::Receiver<()> {
    CHANGES.get_or_init(|| watch::channel(()).0).subscribe()
}
