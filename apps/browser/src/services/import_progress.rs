use gpui::{App, IntoElement, Window};
use gpui_component::{WindowExt, notification::Notification, progress::Progress};

/// The import owns this guard; completion, failure, and cancellation all close
/// its toast, even if the browse page that started the import has been closed.
pub(crate) struct ImportProgressToast(async_channel::Sender<()>);

impl Drop for ImportProgressToast {
    fn drop(&mut self) {
        let _ = self.0.try_send(());
    }
}

impl ImportProgressToast {
    pub(crate) fn start(window: &mut Window, cx: &mut App) -> Self {
        let id = gpui::SharedString::from(uuid::Uuid::new_v4().to_string());
        window.push_notification(Notification::new().id1::<Self>(id.clone()).autohide(false).title("Importing books").content(|_, _, _| Progress::new("import-progress").loading(true).into_any_element()), cx);
        let (done, finished) = async_channel::bounded::<()>(1);
        window
            .spawn(cx, async move |cx| {
                let _ = finished.recv().await;
                let _ = cx.update(|window, cx| window.remove_notification1::<ImportProgressToast>(id, cx));
            })
            .detach();
        Self(done)
    }
}
