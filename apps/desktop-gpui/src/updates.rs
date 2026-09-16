//! Self-update mechanism, parked until a UI is wired back in.
//!
//! The browser UI used to own these types because it rendered the update
//! section in Settings. That section is gone, so they live here with the two
//! platform implementations that build them ([`crate::updater`] for AppImage,
//! [`crate::windows_updater`] for the Windows installer). Nothing calls
//! `check`/`install`/`restart` today; re-wiring means constructing an updater
//! in `lib.rs` and handing it to whatever surface presents updates.

use std::rc::Rc;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DesktopUpdate {
    pub version: String,
    pub release_name: String,
    pub download_url: String,
    pub checksum_url: String,
}

pub(crate) type CheckDesktopUpdate = Rc<dyn Fn() -> async_channel::Receiver<Result<Option<DesktopUpdate>, String>>>;
pub(crate) type InstallDesktopUpdate = Rc<dyn Fn(DesktopUpdate) -> async_channel::Receiver<Result<(), String>>>;
pub(crate) type RestartDesktopUpdate = Rc<dyn Fn() -> Result<(), String>>;

#[derive(Clone)]
pub(crate) struct DesktopUpdater {
    current_version: String,
    check: CheckDesktopUpdate,
    install: InstallDesktopUpdate,
    restart: RestartDesktopUpdate,
}

impl DesktopUpdater {
    pub(crate) fn new(current_version: impl Into<String>, check: CheckDesktopUpdate, install: InstallDesktopUpdate, restart: RestartDesktopUpdate) -> Self {
        Self { current_version: current_version.into(), check, install, restart }
    }

    pub(crate) fn current_version(&self) -> &str {
        &self.current_version
    }

    pub(crate) fn check(&self) -> async_channel::Receiver<Result<Option<DesktopUpdate>, String>> {
        (self.check)()
    }

    pub(crate) fn install(&self, update: DesktopUpdate) -> async_channel::Receiver<Result<(), String>> {
        (self.install)(update)
    }

    pub(crate) fn restart(&self) -> Result<(), String> {
        (self.restart)()
    }
}
