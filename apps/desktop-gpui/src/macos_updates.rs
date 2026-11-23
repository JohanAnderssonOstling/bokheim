//! macOS uses the shared data-only host; its application is installed by the OS.
use browser_ui::UpdateControls;
use gpui::{App, Entity};
use linux_update_host::TaxonomyHost;
use std::path::Path;
use update_client::UpdateError;

pub fn initialize(data: &Path) -> Result<(), UpdateError> {
    let Some(config) = crate::update_binding::startup_config(data)? else {
        return Ok(());
    };
    crate::update_binding::store(TaxonomyHost::open(data, env!("CARGO_PKG_VERSION"), config)?)
}

pub fn bind(cx: &mut App) -> Option<Entity<UpdateControls>> {
    let executable = std::env::current_exe().ok()?;
    crate::update_binding::bind(Some(executable), |_, _| Ok(()), Ok, cx)
}
