//! Kobo actions use the same worker, view model, and settings row as desktop.
use ui_text::text;

use crate::services::updates::{PublishUpdate, UpdateAction, UpdateControls, UpdateEvent, UpdateView, bind_worker};
use gpui::{App, Entity};
use kobo_device::wifi::{self, Phase as WifiPhase};
use kobo_update::{Installation, Release};
use std::time::{Duration, Instant};

struct Host {
    installation: Installation,
    release: Option<Release>,
    view: UpdateView,
}

impl Host {
    fn status(&self) -> UpdateEvent {
        match (&self.release, &self.view) {
            (Some(release), UpdateView::Available) => UpdateEvent::Status(self.view.clone(), format!(text!(settings::bokheim_is_available), release.version)),
            (Some(release), UpdateView::Ready { .. }) => UpdateEvent::Status(self.view.clone(), format!(text!(settings::bokheim_is_ready_to_install), release.version)),
            _ => UpdateEvent::View(self.view.clone()),
        }
    }

    fn work(&mut self, action: Option<UpdateAction>, publish: &PublishUpdate<'_>) -> Result<UpdateView, String> {
        match action {
            None | Some(UpdateAction::Check) => {
                publish(UpdateEvent::View(UpdateView::Checking))?;
                self.release = self.installation.check()?;
                self.view = if self.release.is_some() { UpdateView::Available } else { UpdateView::Current { version: self.installation.version.to_string(), checked: true } };
            }
            Some(UpdateAction::Update) => {
                if std::env::var("BOKHEIM_KOBO_CAN_UPDATE").as_deref() != Ok("1") {
                    return Err(text!(settings::use_the_latest_usb_installer_once_to_enable_updates_on_this_kobo).into());
                }
                let release = self.release.as_ref().ok_or(text!(settings::check_for_updates_before_downloading))?;
                publish(UpdateEvent::View(UpdateView::Downloading { bytes: 0, total: 0 }))?;
                let mut last = Instant::now() - Duration::from_secs(1);
                self.installation.download(release, |bytes, total| {
                    if last.elapsed() >= Duration::from_secs(1) || bytes == total {
                        let _ = publish(UpdateEvent::View(UpdateView::Downloading { bytes, total }));
                        last = Instant::now();
                    }
                })?;
                self.view = UpdateView::Ready { action: UpdateAction::Install };
            }
            Some(UpdateAction::Install) => {
                if std::env::var("BOKHEIM_KOBO_CAN_UPDATE").as_deref() != Ok("1") || self.installation.pending().is_none() {
                    return Err(text!(settings::restart_the_app_with_the_latest_kobo_launcher_then_download_the_update).into());
                }
                let path = std::env::var_os("BOKHEIM_KOBO_EXIT_ACTION_FILE").ok_or(text!(settings::the_kobo_launcher_is_unavailable))?;
                std::fs::write(path, "update").map_err(|error| error.to_string())?;
                self.view = UpdateView::Activating;
            }
            _ => {}
        }
        publish(self.status())?;
        Ok(self.view.clone())
    }
}

pub(super) fn bind(cx: &mut App) -> Entity<UpdateControls> {
    let installation = kobo_update::installation().cloned();
    let release = installation.as_ref().ok().and_then(|installation| installation.pending());
    let view =
        if release.is_some() { UpdateView::Ready { action: UpdateAction::Install } } else { UpdateView::Current { version: installation.as_ref().map(|installation| installation.version.to_string()).unwrap_or_default(), checked: false } };
    let error = installation.as_ref().err().cloned().or_else(|| installation.as_ref().ok().and_then(|installation| installation.last_error()));
    let automatic_check = release.is_none();
    let host = installation.ok().map(|installation| Host { installation, release, view: view.clone() });
    let initial = host.as_ref().map(Host::status);
    let controls = bind_worker(view, None, automatic_check, move || host, Host::work, |host| host.view.clone(), |error| Some(error.to_owned()), cx);
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
