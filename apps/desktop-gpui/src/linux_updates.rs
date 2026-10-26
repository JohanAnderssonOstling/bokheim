//! GPUI binding for the Linux startup/update host. Activation belongs to startup.
pub use crate::update_binding::{bind_desktop as bind, healthy, trial_pending};
use linux_update_host::Host;
pub use linux_update_host::Startup;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

pub use crate::update_entry::candidate_entry;

pub fn initialize(data: &Path) -> Result<Startup, String> {
    let Some(config) = crate::update_binding::startup_config(data)? else {
        return Ok(Startup::Run);
    };
    let appimage = std::env::var_os("APPIMAGE").filter(|_| std::env::var_os("BOKHEIM_DISABLE_SELF_UPDATE").is_none()).map(PathBuf::from).map(|p| p.canonicalize().map_err(|e| e.to_string())).transpose()?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut host = Host::open(data, env!("CARGO_PKG_VERSION"), config, appimage, executable)?;
    let args = std::env::args().collect::<Vec<_>>();
    let trial = args.windows(2).find(|a| a[0] == "--bokheim-update-trial").map(|a| a[1].as_str());
    let startup = host.startup(trial)?;
    if matches!(startup, Startup::Run) {
        crate::update_binding::store(host)?;
    }
    Ok(startup)
}

/// Parent waits only during a new launch, while no previous user session exists.
/// Failure returns to the old process, whose next startup recovers the journal.
pub fn launch(data: &Path, executable: &Path, trial: Option<&str>) -> Result<bool, String> {
    let mut command = std::process::Command::new(executable);
    command.env_remove("APPIMAGE").env_remove("APPDIR");
    if let Some(id) = trial {
        command.arg("--bokheim-update-trial").arg(id);
    }
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(error) => {
            log::warn!("could not launch update: {error}");
            return Ok(false);
        }
    };
    let Some(id) = trial else { return Ok(true) };
    match update_client::supervisor::wait_for_trial(&mut child, &data.join("updates"), id, Duration::from_secs(120))? {
        update_client::supervisor::Outcome::Committed => Ok(true),
        update_client::supervisor::Outcome::Stopped(reason) => {
            log::warn!("startup trial stopped; checking recovery journal: {reason}");
            Ok(false)
        }
    }
}
