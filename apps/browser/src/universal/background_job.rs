//! Dialog lifecycle shared by import and export operations.
use gpui::prelude::*;
use gpui::{App, Context, Render, Task, Window, px};
use gpui_component::WindowExt;
use library_backend::{BackendError, LibraryClient};
use std::time::Duration;
use uuid::Uuid;

pub(super) trait BackgroundJobView: Sized + 'static {
    type Progress: 'static;
    fn library(&self) -> &LibraryClient;
    fn active(&self) -> Option<Uuid>;
    fn cancelling(&self) -> bool;
    fn receive_progress(&mut self, progress: Self::Progress);
    fn confirm_job(&mut self, cx: &mut Context<Self>) -> bool;
    fn close_job(&mut self, cx: &mut Context<Self>) -> bool;
    async fn cancel(library: &LibraryClient, id: Uuid) -> Result<bool, BackendError>;
    async fn progress(library: &LibraryClient, id: Uuid) -> Result<Option<Self::Progress>, BackendError>;
}

pub(super) fn cancel_on_release<V: BackgroundJobView>(cx: &mut Context<V>) {
    cx.on_release(|view, cx| {
        if let Some(id) = view.active() {
            let library = view.library().clone();
            cx.foreground_executor()
                .spawn(async move {
                    let _ = V::cancel(&library, id).await;
                })
                .detach();
        }
    })
    .detach();
}

pub(super) fn poll_job<V: BackgroundJobView>(id: Uuid, library: LibraryClient, cx: &mut Context<V>) -> Task<()> {
    cx.spawn(async move |view, cx| {
        let mut cancel_sent = false;
        loop {
            cx.background_executor().timer(Duration::from_millis(150)).await;
            let Ok(Some(cancelling)) = view.update(cx, |view, _| (view.active() == Some(id)).then_some(view.cancelling())) else { break };
            // Retry until registration, including cancellation before admission.
            if cancelling && !cancel_sent {
                cancel_sent = V::cancel(&library, id).await.unwrap_or(false);
            }
            if let Ok(Some(progress)) = V::progress(&library, id).await {
                let _ = view.update(cx, |view, cx| {
                    if view.active() == Some(id) {
                        view.receive_progress(progress);
                        cx.notify();
                    }
                });
            }
        }
    })
}

pub(super) fn open_job_dialog<V: BackgroundJobView + Render>(max_width: f32, build: impl FnOnce(&mut Window, &mut Context<V>) -> V + 'static, window: &mut Window, cx: &mut App) {
    // Popup dismissal restores trigger focus before the new dialog takes focus.
    window.defer(cx, move |window, cx| {
        window.defer(cx, move |window, cx| {
            let view = cx.new(|cx| build(window, cx));
            window.open_dialog(cx, move |dialog, window, _| {
                let cancel = view.clone();
                let confirm = view.clone();
                let width = (f32::from(window.viewport_size().width) - 2.0 * ui_components::SPACE_MD).clamp(1.0, max_width);
                dialog
                    .w(px(width))
                    .p_0()
                    .close_button(false)
                    .overlay_closable(false)
                    .on_cancel(move |_, _, cx| cancel.update(cx, |view, cx| view.close_job(cx)))
                    .on_ok(move |_, _, cx| confirm.update(cx, |view, cx| view.confirm_job(cx)))
                    .child(view.clone())
            });
        })
    });
}
