//! Kobo update actions and the handoff to the firmware launcher.
use crate::{Installation, Release};
use semver::Version;
use std::time::{Duration, Instant};
use update_client::UpdateError as OperationError;
use update_client::coordinator::{UpdateAction, UpdateView};

#[derive(Debug)]
pub enum UpdateError {
    LauncherUpgradeRequired,
    CheckRequired,
    RestartRequired,
    LauncherUnavailable,
    Failed(OperationError),
}

impl From<OperationError> for UpdateError {
    fn from(error: OperationError) -> Self {
        Self::Failed(error)
    }
}

pub struct Host {
    installation: Installation,
    release: Option<Release>,
    view: UpdateView,
    taxonomy: Option<linux_update_host::TaxonomyHost>,
}

impl Host {
    pub fn new(installation: Installation) -> Self {
        let release = installation.pending();
        let view = if release.is_some() { UpdateView::Ready { action: UpdateAction::Install } } else { UpdateView::Current { version: installation.version.to_string(), checked: false } };
        let taxonomy = installation.taxonomy.lock().ok().and_then(|mut slot| slot.take());
        Self { installation, release, view, taxonomy }
    }

    pub fn view(&self) -> UpdateView {
        if self.application_has_priority() {
            return self.view.clone();
        }
        self.taxonomy.as_ref().map(|host| host.taxonomy_view()).filter(|view| *view != UpdateView::Hidden).unwrap_or_else(|| self.view.clone())
    }

    pub fn release_version(&self) -> Option<&Version> {
        self.application_has_priority().then(|| self.release.as_ref().map(|release| &release.version)).flatten()
    }

    fn application_has_priority(&self) -> bool {
        !matches!(self.view, UpdateView::Current { .. } | UpdateView::Hidden | UpdateView::Checking)
    }

    /// Publish intermediate progress; the returned view is the completed action.
    pub fn work(&mut self, action: Option<UpdateAction>, mut publish: impl FnMut(UpdateView) -> Result<(), UpdateError>) -> Result<UpdateView, UpdateError> {
        match action {
            None | Some(UpdateAction::Check) => {
                publish(UpdateView::Checking)?;
                let application = self.installation.check();
                let taxonomy = self.taxonomy.as_mut().map(|host| host.check_for_updates());
                self.release = match application {
                    Ok(release) => release,
                    Err(error) if self.taxonomy.as_ref().is_some_and(|host| host.taxonomy_view() != UpdateView::Hidden) => {
                        let _ = error;
                        None
                    }
                    Err(error) => return Err(error.into()),
                };
                if self.release.is_none() {
                    if let Some(result) = taxonomy {
                        result?;
                    }
                }
                if let Some(host) = self.taxonomy.as_mut() {
                    let prepared = host.prepare();
                    if self.release.is_none() {
                        prepared?;
                    }
                }
                self.view = if self.release.is_some() { UpdateView::Available } else { UpdateView::Current { version: self.installation.version.to_string(), checked: true } };
            }
            Some(UpdateAction::Update) if !self.application_has_priority() => {
                let host = self.taxonomy.as_mut().ok_or(UpdateError::CheckRequired)?;
                host.approve()?;
                publish(host.taxonomy_view())?;
                host.prepare()?;
            }
            Some(UpdateAction::Restart) if !self.application_has_priority() => {
                if !self.taxonomy.as_ref().is_some_and(|host| matches!(host.taxonomy_view(), UpdateView::Ready { action: UpdateAction::Restart })) {
                    return Err(UpdateError::CheckRequired);
                }
                let path = std::env::var_os("BOKHEIM_KOBO_EXIT_ACTION_FILE").ok_or(UpdateError::LauncherUnavailable)?;
                std::fs::write(path, "restart").map_err(OperationError::operation)?;
                self.view = UpdateView::Activating;
            }
            Some(UpdateAction::Update) => {
                if !launcher_can_update() {
                    return Err(UpdateError::LauncherUpgradeRequired);
                }
                let release = self.release.as_ref().ok_or(UpdateError::CheckRequired)?;
                publish(UpdateView::Downloading { bytes: 0, total: 0 })?;
                let mut last = Instant::now() - Duration::from_secs(1);
                self.installation.download(release, |bytes, total| {
                    if last.elapsed() >= Duration::from_secs(1) || bytes == total {
                        let _ = publish(UpdateView::Downloading { bytes, total });
                        last = Instant::now();
                    }
                })?;
                self.view = UpdateView::Ready { action: UpdateAction::Install };
            }
            Some(UpdateAction::Install) => {
                if !launcher_can_update() || self.installation.pending().is_none() {
                    return Err(UpdateError::RestartRequired);
                }
                let path = std::env::var_os("BOKHEIM_KOBO_EXIT_ACTION_FILE").ok_or(UpdateError::LauncherUnavailable)?;
                std::fs::write(path, "update").map_err(|error| UpdateError::Failed(OperationError::operation(error)))?;
                self.view = UpdateView::Activating;
            }
            _ => {}
        }
        Ok(self.view())
    }
}

fn launcher_can_update() -> bool {
    std::env::var("BOKHEIM_KOBO_CAN_UPDATE").as_deref() == Ok("1")
}

impl Drop for Host {
    fn drop(&mut self) {
        if let Some(taxonomy) = self.taxonomy.take() {
            if let Ok(mut slot) = self.installation.taxonomy.lock() {
                *slot = Some(taxonomy);
            }
        }
    }
}
