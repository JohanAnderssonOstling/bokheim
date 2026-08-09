//! Formats account sessions and owns their revocation operations.

use std::sync::Arc;

use epub_svelte_lib::app::AccountSessionSummary;
use gpui::Context;

use super::SyncPage;
use super::account::load_cloud_snapshot;

#[derive(Default)]
pub(super) struct SessionsState {
    pub(super) items: Arc<Vec<AccountSessionSummary>>,
}

pub(super) fn detail(session: &AccountSessionSummary) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_secs_f64()).unwrap_or(session.created_at);
    format!("Created {} ago · expires in {}", duration_label(now - session.created_at), duration_label(session.expires_at - now))
}

fn duration_label(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    if seconds >= 86_400.0 {
        format!("{} days", (seconds / 86_400.0).floor() as u64)
    } else if seconds >= 3_600.0 {
        format!("{} hours", (seconds / 3_600.0).floor() as u64)
    } else {
        format!("{} minutes", (seconds / 60.0).floor() as u64)
    }
}

impl SyncPage {
    pub(super) fn start_revoke_session(&mut self, session_id: String, cx: &mut Context<Self>) {
        self.start_snapshot_operation(
            "Revoking session…",
            "Session revoked",
            move |core| async move {
                core.revoke_account_session(&session_id).await.map_err(|error| error.to_string())?;
                load_cloud_snapshot(&core).await
            },
            cx,
        );
    }

    pub(super) fn start_revoke_other_sessions(&mut self, cx: &mut Context<Self>) {
        self.start_snapshot_operation(
            "Signing out other sessions…",
            "Other sessions signed out",
            |core| async move {
                core.revoke_other_account_sessions().await.map_err(|error| error.to_string())?;
                load_cloud_snapshot(&core).await
            },
            cx,
        );
    }
}
