//! Windows desktop binding. Live files change only in the external startup helper.
pub use crate::update_binding::{bind_desktop as bind, healthy, trial_pending};
use linux_update_host::windows::{WindowsHost, WindowsStartup};
pub use linux_update_host::windows_process::{Entry, enter};
use std::path::Path;

/// Register for this user only, with no administrator privileges.
pub fn register_reader_scheme() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let command = format!("\"{}\" \"%1\"", executable.display());
    let key = r"HKCU\Software\Classes\bokheim";
    let open = format!(r"{key}\shell\open\command");
    let existing = Command::new("reg.exe").creation_flags(0x08000000).args(["query", &open, "/ve"]).output().map_err(|error| error.to_string())?;
    if existing.status.success() && String::from_utf8_lossy(&existing.stdout).contains(&command) {
        return Ok(());
    }
    for arguments in [vec!["add", key, "/ve", "/d", "URL:Bokheim reader", "/f"], vec!["add", key, "/v", "URL Protocol", "/d", "", "/f"], vec!["add", &open, "/ve", "/d", &command, "/f"]] {
        let result = Command::new("reg.exe").creation_flags(0x08000000).args(arguments).output().map_err(|error| error.to_string())?;
        if !result.status.success() {
            return Err(String::from_utf8_lossy(&result.stderr).into_owned());
        }
    }
    Ok(())
}

pub fn helper_entry(data: &Path) -> Result<bool, String> {
    linux_update_host::windows_process::helper_entry(data, env!("CARGO_PKG_VERSION"), crate::update_binding::discovery_config)
}

/// False means the copied helper is ready and this process must exit.
pub fn initialize(data: &Path, trial: Option<&str>, startup_arguments: &[String]) -> Result<bool, String> {
    let Some(config) = crate::update_binding::startup_config(data)? else {
        return Ok(true);
    };
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let installation = executable.parent().ok_or("executable has no directory")?;
    let mut host = WindowsHost::open(data, installation, env!("CARGO_PKG_VERSION"), config)?;
    if matches!(host.startup(trial)?, WindowsStartup::Helper) {
        match linux_update_host::windows_process::spawn_helper_with_arguments(data, installation, startup_arguments) {
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
