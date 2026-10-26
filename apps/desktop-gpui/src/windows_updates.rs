//! Windows desktop binding. Live files change only in the external startup helper.
pub use crate::update_binding::{bind_desktop as bind, healthy, trial_pending};
use linux_update_host::windows::{WindowsHost, WindowsStartup};
pub use linux_update_host::windows_process::{Entry, enter};
use std::path::Path;

pub fn helper_entry(data: &Path) -> Result<bool, String> {
    linux_update_host::windows_process::helper_entry(data, env!("CARGO_PKG_VERSION"), crate::update_binding::discovery_config)
}

/// False means the copied helper is ready and this process must exit.
pub fn initialize(data: &Path, trial: Option<&str>) -> Result<bool, String> {
    let Some(config) = crate::update_binding::startup_config(data)? else {
        return Ok(true);
    };
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let installation = executable.parent().ok_or("executable has no directory")?;
    let mut host = WindowsHost::open(data, installation, env!("CARGO_PKG_VERSION"), config)?;
    if matches!(host.startup(trial)?, WindowsStartup::Helper) {
        match linux_update_host::windows_process::spawn_helper(data, installation) {
            Ok(()) => return Ok(false),
            Err(error) => {
                let required = std::fs::metadata(&executable).map_err(|e| e.to_string())?.len();
                let available = fs2::available_space(data).map_err(|e| e.to_string())?;
                let reason = if required > available { update_client::coordinator::WaitReason::Space { additional_bytes: required - available } } else { update_client::coordinator::WaitReason::Retry };
                host.helper_unavailable(reason, error)?;
                if !matches!(host.startup(None)?, WindowsStartup::Run) {
                    return Err("update recovery requires its helper".into());
                }
            }
        }
    }
    crate::update_binding::store(host)?;
    Ok(true)
}
