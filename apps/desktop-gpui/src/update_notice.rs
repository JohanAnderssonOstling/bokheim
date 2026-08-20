//! Desktop-owned automatic update check and notification.

use browser_ui::{DesktopUpdater, ShowSettings};
use gpui::{AnyWindowHandle, App};
use gpui_component::WindowExt;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::notification::Notification;

struct DesktopUpdateNotice;

pub(crate) fn check_for_update(updater: Option<DesktopUpdater>, window_handle: AnyWindowHandle, cx: &mut App) {
    let Some(updater) = updater else { return };
    let update_check = updater.check();
    cx.spawn(async move |cx| {
        let outcome = update_check.recv().await.unwrap_or_else(|_| Err("The desktop update worker stopped unexpectedly".to_owned()));
        cx.update(|cx| match outcome {
            Ok(Some(update)) => {
                updater.set_available_update(Some(update.clone()));
                let version = update.version;
                let _ = window_handle.update(cx, |_, window, cx| {
                    window.push_notification(
                        Notification::info(format!("Bokheim {version} is ready to download.")).id1::<DesktopUpdateNotice>("desktop-update-available").title("Update available").action(move |_, _, notification_cx| {
                            Button::new("view-desktop-update").primary().label("View update").on_click(notification_cx.listener(move |notification, _, window, cx| {
                                notification.dismiss(window, cx);
                                window.dispatch_action(Box::new(ShowSettings), cx);
                            }))
                        }),
                        cx,
                    );
                });
            }
            Ok(None) => updater.set_available_update(None),
            Err(error) => log::warn!("automatic desktop update check failed: {error}"),
        });
    })
    .detach();
}
