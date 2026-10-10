//! Library reconciliation driven by the shared account notification connection.

use super::LibraryDirectory;
use crate::integration::{InterestState, LibraryAccount};
use crate::BackendError;
use std::sync::Arc;
use std::time::Duration;
use sync_common::api::notifications::NotificationMessage;
use web_time::Instant;

/// Routes notification events to open library actors and reconciles on resume.
/// The application supplies activity and retains account discovery and transport.
pub struct LibraryNotifications {
    libraries: Arc<LibraryDirectory>,
    account: LibraryAccount,
    foreground: bool,
    generation: u64,
    refreshed: Instant,
}

impl LibraryNotifications {
    pub fn new(libraries: Arc<LibraryDirectory>, account: LibraryAccount, activity: InterestState) -> Self {
        Self { libraries, account, foreground: activity.foreground(), generation: activity.resume_generation(), refreshed: Instant::now() }
    }

    /// Returns whether account discovery should also reconcile after a resume.
    /// Library refresh does not depend on a successful socket handshake.
    pub fn observe_activity(&mut self, activity: InterestState) -> bool {
        self.observe_activity_at(activity, Instant::now())
    }

    pub(super) fn observe_activity_at(&mut self, activity: InterestState, now: Instant) -> bool {
        let resumed = activity.foreground() && (!self.foreground || activity.resume_generation() != self.generation);
        self.foreground = activity.foreground();
        self.generation = activity.resume_generation();
        if !resumed || now.duration_since(self.refreshed) < Duration::from_secs(30) {
            return false;
        }
        self.refreshed = now;
        if self.account.current().ok().flatten().is_none() {
            return false;
        }
        if let Err(error) = self.libraries.request_remote_refresh_all() {
            log::warn!("could not refresh libraries after foreground resume: {error}");
        }
        true
    }

    /// Hello reconciles after connecting or reconnecting while browsing.
    /// An event for an unopened library waits for that library's initial sync.
    pub fn receive(&self, message: &NotificationMessage, browsing: bool) -> Result<(), BackendError> {
        match message {
            NotificationMessage::Hello { .. } if !browsing => Ok(()),
            NotificationMessage::LibraryChanged { library_id } => self.libraries.request_remote_refresh(library_id),
            NotificationMessage::LibrariesChanged | NotificationMessage::Hello { .. } | NotificationMessage::Reconcile => self.libraries.request_remote_refresh_all(),
        }
    }
}
