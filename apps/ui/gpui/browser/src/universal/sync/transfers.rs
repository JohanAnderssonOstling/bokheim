//! Holds transfer summaries and coalesces transfer notifications into refreshes.

use std::sync::Arc;

use bokheim_backend::LibraryTransfers;
use gpui::Context;

use super::SyncPage;

#[derive(Default)]
pub(super) struct TransferState {
    pub(super) groups: Arc<Vec<LibraryTransfers>>,
}

impl SyncPage {
    /// Coalesces bursts of backend transfer notifications.
    pub(crate) fn schedule_refresh(&mut self, cx: &mut Context<Self>) {
        if self.refresh_scheduled {
            return;
        }
        self.refresh_scheduled = true;
        let timer = cx.background_executor().timer(std::time::Duration::from_millis(200));
        cx.spawn(async move |page, cx| {
            timer.await;
            let _ = page.update(cx, |page, cx| {
                page.refresh_scheduled = false;
                page.refresh_with_feedback(false, cx);
            });
        })
        .detach();
    }
}
