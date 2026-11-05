//! Kobo actions use the same worker, view model, and settings row as desktop.
use ui_text::text;

use crate::services::updates::{PublishUpdate, UpdateAction, UpdateControls, UpdateEvent, UpdateView, bind_worker};
use gpui::{App, Entity};
use kobo_device::wifi::{self, Phase as WifiPhase};
use kobo_update::{Host, UpdateError};

fn status(host: &Host) -> UpdateEvent {
    let view = host.view();
    match (host.release_version(), &view) {
        (Some(version), UpdateView::Available) => UpdateEvent::Status(view, format!(text!(settings::bokheim_is_available), version)),
        (Some(version), UpdateView::Ready { .. }) => UpdateEvent::Status(view, format!(text!(settings::bokheim_is_ready_to_install), version)),
        _ => UpdateEvent::View(view),
    }
}

fn work(host: &mut Host, action: Option<UpdateAction>, publish: &PublishUpdate<'_>) -> Result<UpdateView, String> {
    let view = host.work(action, |view| publish(UpdateEvent::View(view))).map_err(|error| match error {
        UpdateError::LauncherUpgradeRequired => text!(settings::use_the_latest_usb_installer_once_to_enable_updates_on_this_kobo).into(),
        UpdateError::CheckRequired => text!(settings::check_for_updates_before_downloading).into(),
        UpdateError::RestartRequired => text!(settings::restart_the_app_with_the_latest_kobo_launcher_then_download_the_update).into(),
        UpdateError::LauncherUnavailable => text!(settings::the_kobo_launcher_is_unavailable).into(),
        UpdateError::Failed(error) => error,
    })?;
    publish(status(host))?;
    Ok(view)
}

pub(super) fn bind(cx: &mut App) -> Entity<UpdateControls> {
    let installation = kobo_update::installation().cloned();
    let error = installation.as_ref().err().cloned().or_else(|| installation.as_ref().ok().and_then(|installation| installation.last_error()));
    let host = installation.ok().map(Host::new);
    let view = host.as_ref().map(Host::view).unwrap_or(UpdateView::Current { version: String::new(), checked: false });
    let automatic_check = !matches!(view, UpdateView::Ready { .. });
    let initial = host.as_ref().map(status);
    let controls = bind_worker(view, None, automatic_check, move || host, work, Host::view, |error| Some(error.to_owned()), cx);
    controls.update(cx, |controls, cx| {
        controls.action_labels(text!(settings::download_update), text!(settings::restart_and_install));
        if let Some(initial) = initial {
            controls.apply_event(initial, cx);
        }
        if let Some(error) = error {
            controls.apply_event(UpdateEvent::Failed(Some(error)), cx);
        }
    });
    // An offline startup must not leave updates undiscoverable now that the
    // manual check row is hidden. Retry when Wi-Fi becomes connected.
    if let Ok(mut changes) = wifi::subscribe() {
        let weak = controls.downgrade();
        cx.spawn(async move |cx| {
            let mut connected = changes.borrow_and_update().phase == WifiPhase::Connected;
            while changes.changed().await.is_ok() {
                let next_connected = changes.borrow_and_update().phase == WifiPhase::Connected;
                if next_connected && !connected {
                    if weak
                        .update(cx, |controls, cx| {
                            if matches!(controls.view, UpdateView::Current { .. }) {
                                (controls.action)(UpdateAction::Check, cx);
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
                connected = next_connected;
            }
        })
        .detach();
    }
    // Installation still belongs to the firmware launcher, after GPUI exits.
    let weak = controls.downgrade();
    cx.observe(&controls, move |_, cx| {
        if weak.upgrade().is_some_and(|controls| controls.read(cx).view == UpdateView::Activating) {
            cx.quit();
        }
    })
    .detach();
    controls
}
