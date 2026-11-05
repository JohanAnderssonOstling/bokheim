//! Library reconciliation driven by the shared account notification connection.

use super::LibraryHandle;
use library_services::account::LibraryAccount;
use library_services::interest::InterestState;
use std::time::Duration;
use sync_common::api::notifications::NotificationMessage;
use web_time::Instant;

/// Routes notification events to open library services and reconciles on resume.
/// The application supplies activity and retains account discovery and transport.
pub struct LibraryNotifications {
    account: LibraryAccount,
    foreground: bool,
    generation: u64,
    refreshed: Instant,
}

impl LibraryNotifications {
    pub fn new(account: LibraryAccount, activity: InterestState) -> Self {
        Self { account, foreground: activity.foreground(), generation: activity.resume_generation(), refreshed: Instant::now() }
    }

    /// Returns whether account discovery should also reconcile after a resume.
    /// Library refresh does not depend on a successful socket handshake.
    pub async fn observe_activity(&mut self, activity: InterestState, libraries: &[LibraryHandle]) -> bool {
        self.observe_activity_at(activity, Instant::now(), libraries).await
    }

    pub(super) async fn observe_activity_at(&mut self, activity: InterestState, now: Instant, libraries: &[LibraryHandle]) -> bool {
        let resumed = activity.foreground() && (!self.foreground || activity.resume_generation() != self.generation);
        self.foreground = activity.foreground();
        self.generation = activity.resume_generation();
        if !resumed || now.duration_since(self.refreshed) < Duration::from_secs(30) {
            return false;
        }
        self.refreshed = now;
        if self.account.session().await.ok().flatten().is_none() {
            return false;
        }
        for library in libraries {
            library.request_remote_refresh();
        }
        true
    }

    /// Hello reconciles after connecting or reconnecting while browsing.
    /// An event for an unopened library waits for that library's initial sync.
    pub fn receive(&self, message: &NotificationMessage, browsing: bool, libraries: &[LibraryHandle]) {
        if matches!(message, NotificationMessage::Hello { .. }) && !browsing {
            return;
        }
        for library in libraries {
            if let NotificationMessage::LibraryChanged { library_id } = message {
                if library.id() != library_id {
                    continue;
                }
            }
            library.request_remote_refresh();
        }
    }
}
