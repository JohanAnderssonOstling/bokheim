use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The helper uses the distribution's WebKitGTK, outside the AppImage's
/// compatibility library path. Preserve any path supplied before AppRun.
pub(super) fn command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    if cfg!(target_os = "linux") {
        if let Some(path) = std::env::var_os("BOKHEIM_SYSTEM_LD_LIBRARY_PATH") {
            command.env("LD_LIBRARY_PATH", path);
        }
    }
    command
}

/// Missing libraries and incompatible glibc fail in the helper's loader,
/// leaving the main app running. A broken helper must not stall discovery.
pub(super) fn probe(executable: &Path, timeout: Duration) -> bool {
    let Ok(mut child) = command(executable).arg("probe").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else {
        return false;
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if started.elapsed() < timeout => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn missing_or_incompatible_helpers_are_unavailable() {
        let directory = tempfile::tempdir().unwrap();
        let helper = directory.path().join("browser");
        assert!(!probe(&helper, Duration::from_secs(1)));
        fs::write(&helper, "#!/bin/sh\nexit 127\n").unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(!probe(&helper, Duration::from_secs(1)));
        fs::write(&helper, "#!/bin/sh\nexit 0\n").unwrap();
        assert!(probe(&helper, Duration::from_secs(1)));
    }

    #[test]
    fn stalled_helpers_are_terminated_and_reaped() {
        let directory = tempfile::tempdir().unwrap();
        let helper = directory.path().join("browser");
        let pid = directory.path().join("pid");
        fs::write(&helper, format!("#!/bin/sh\necho $$ > '{}'\nexec sleep 30\n", pid.display())).unwrap();
        fs::set_permissions(&helper, fs::Permissions::from_mode(0o755)).unwrap();
        let started = Instant::now();
        assert!(!probe(&helper, Duration::from_millis(100)));
        assert!(started.elapsed() < Duration::from_secs(2));
        #[cfg(target_os = "linux")]
        assert!(!Path::new("/proc").join(fs::read_to_string(pid).unwrap().trim()).exists());
    }
}
