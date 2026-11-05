//! Kobo update actions and the handoff to the firmware launcher.
use crate::{Installation, Release};
use semver::Version;
use std::time::{Duration, Instant};
use update_client::coordinator::{UpdateAction, UpdateView};

#[derive(Debug)]
pub enum UpdateError {
    LauncherUpgradeRequired,
    CheckRequired,
    RestartRequired,
    LauncherUnavailable,
    Failed(String),
}

impl From<String> for UpdateError {
    fn from(error: String) -> Self {
        Self::Failed(error)
    }
}

pub struct Host {
    installation: Installation,
    release: Option<Release>,
    view: UpdateView,
}

impl Host {
    pub fn new(installation: Installation) -> Self {
        let release = installation.pending();
        let view = if release.is_some() { UpdateView::Ready { action: UpdateAction::Install } } else { UpdateView::Current { version: installation.version.to_string(), checked: false } };
        Self { installation, release, view }
    }

    pub fn view(&self) -> UpdateView {
        self.view.clone()
    }

    pub fn release_version(&self) -> Option<&Version> {
        self.release.as_ref().map(|release| &release.version)
    }

    /// Publish intermediate progress; the returned view is the completed action.
    pub fn work(&mut self, action: Option<UpdateAction>, mut publish: impl FnMut(UpdateView) -> Result<(), String>) -> Result<UpdateView, UpdateError> {
        match action {
            None | Some(UpdateAction::Check) => {
                publish(UpdateView::Checking)?;
                self.release = self.installation.check()?;
                self.view = if self.release.is_some() { UpdateView::Available } else { UpdateView::Current { version: self.installation.version.to_string(), checked: true } };
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
                std::fs::write(path, "update").map_err(|error| UpdateError::Failed(error.to_string()))?;
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
