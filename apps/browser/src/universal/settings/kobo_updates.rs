//! Kobo actions use the same worker, view model, and settings row as desktop.
use crate::services::updates::{PublishUpdate, UpdateAction, UpdateControls, UpdateEvent, UpdateView, bind_worker};
use gpui::{App, Entity};
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
            (Some(release), UpdateView::Available) => UpdateEvent::Status(self.view.clone(), format!("Bokheim {} is available", release.version)),
            (Some(release), UpdateView::Ready { .. }) => UpdateEvent::Status(self.view.clone(), format!("Bokheim {} is ready to install", release.version)),
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
                    return Err("Install Bokheim once with the latest USB installer to enable updates on this Kobo.".into());
                }
                let release = self.release.as_ref().ok_or("Check for updates before downloading")?;
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
                    return Err("Restart Bokheim with the latest Kobo launcher and download the update again.".into());
                }
                let path = std::env::var_os("BOKHEIM_KOBO_EXIT_ACTION_FILE").ok_or("The Kobo launcher is unavailable")?;
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
    let host = installation.ok().map(|installation| Host { installation, release, view: view.clone() });
    let initial = host.as_ref().map(Host::status);
    let controls = bind_worker(view, None, false, move || host, Host::work, |host| host.view.clone(), |error| Some(error.to_owned()), cx);
    controls.update(cx, |controls, cx| {
        controls.action_labels("Download update", "Restart and install");
        if let Some(initial) = initial {
            controls.apply_event(initial, cx);
        }
        if let Some(error) = error {
            controls.apply_event(UpdateEvent::Failed(Some(error)), cx);
        }
    });
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
