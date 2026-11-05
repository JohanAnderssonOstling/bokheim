//! Foreground presentation is independent of admission to protected state.
use library_model::{BookFileActivity, LibraryBusyReason};
use tokio::sync::watch;

pub(crate) struct LibraryActivity {
    active: watch::Sender<Vec<LibraryBusyReason>>,
}
impl LibraryActivity {
    pub(crate) fn new() -> Self {
        Self { active: watch::channel(Vec::new()).0 }
    }
    pub(crate) fn snapshot(&self, downloading: bool) -> BookFileActivity {
        let active = self.active.borrow();
        BookFileActivity::from_reasons(&active, downloading)
    }
    pub(crate) fn subscribe(&self) -> watch::Receiver<Vec<LibraryBusyReason>> {
        self.active.subscribe()
    }
    /// Starts before awaiting the library state lock, so queued work is visible too.
    pub(crate) fn start(&self, reason: Option<LibraryBusyReason>) -> Option<ForegroundActivity> {
        let reason = reason?;
        self.active.send_modify(|active| active.push(reason));
        Some(ForegroundActivity { active: self.active.clone(), reason })
    }
}
pub(crate) struct ForegroundActivity {
    active: watch::Sender<Vec<LibraryBusyReason>>,
    reason: LibraryBusyReason,
}
impl Drop for ForegroundActivity {
    fn drop(&mut self) {
        self.active.send_modify(|active| {
            if let Some(index) = active.iter().position(|reason| *reason == self.reason) {
                active.remove(index);
            }
        });
    }
}
