//! Safely release a mounted Kobo after all installation files are verified.
use std::io;
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EjectOutcome {
    Ejected,
    /// Unmounted safely, but the host could not power off/eject the USB device.
    Unmounted,
}

pub fn eject_device(root: &Path) -> io::Result<EjectOutcome> {
    let device = super::KoboDevice::from_mount(root)?;
    eject_mounted_device(device.root())
}

#[cfg(target_os = "linux")]
fn eject_mounted_device(root: &Path) -> io::Result<EjectOutcome> {
    eject_linux(root, &mut |program, arguments| std::process::Command::new(program).args(arguments).output())
}

#[cfg(target_os = "linux")]
fn eject_linux(root: &Path, run: &mut impl FnMut(&str, &[std::ffi::OsString]) -> io::Result<std::process::Output>) -> io::Result<EjectOutcome> {
    use std::ffi::OsString;
    let mount = run("findmnt", &["--noheadings".into(), "--output".into(), "SOURCE".into(), "--mountpoint".into(), root.as_os_str().to_owned()])?;
    let source = String::from_utf8(mount.stdout).map_err(io::Error::other)?;
    let source = source.trim();
    if !mount.status.success() || !source.starts_with("/dev/") || source.contains('\n') || source.contains('[') {
        return Err(io::Error::other("Kobo path is not a mounted block device; eject it manually"));
    }
    let sync = run("sync", &["--file-system".into(), root.as_os_str().to_owned()])?;
    if !sync.status.success() {
        return Err(command_error("sync Kobo", &sync.stderr));
    }
    let args: [OsString; 4] = ["unmount".into(), "--block-device".into(), source.into(), "--no-user-interaction".into()];
    let unmount = run("udisksctl", &args)?;
    if !unmount.status.success() {
        return Err(command_error("unmount Kobo", &unmount.stderr));
    }
    let mut args = args;
    args[0] = "power-off".into();
    match run("udisksctl", &args) {
        Ok(output) if output.status.success() => Ok(EjectOutcome::Ejected),
        _ => Ok(EjectOutcome::Unmounted),
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn command_error(operation: &str, stderr: &[u8]) -> io::Error {
    io::Error::other(format!("Could not {operation}: {}", String::from_utf8_lossy(stderr).trim()))
}

#[cfg(target_os = "macos")]
fn eject_mounted_device(root: &Path) -> io::Result<EjectOutcome> {
    let output = std::process::Command::new("diskutil").arg("eject").arg(root).output()?;
    if !output.status.success() {
        return Err(command_error("eject Kobo", &output.stderr));
    }
    Ok(EjectOutcome::Ejected)
}

#[cfg(target_os = "windows")]
fn eject_mounted_device(root: &Path) -> io::Result<EjectOutcome> {
    use std::fs::OpenOptions;
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use std::path::{Component, Prefix};
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};
    use windows_sys::Win32::System::Ioctl::{FSCTL_DISMOUNT_VOLUME, FSCTL_LOCK_VOLUME, FSCTL_UNLOCK_VOLUME, IOCTL_STORAGE_EJECT_MEDIA};
    use windows_sys::Win32::System::IO::DeviceIoControl;

    let letter = match root.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
            _ => return Err(io::Error::other("Kobo is not mounted as a drive letter")),
        },
        _ => return Err(io::Error::other("Kobo is not mounted as a drive letter")),
    };
    let volume = OpenOptions::new().read(true).write(true).share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE).open(format!("\\\\.\\{}:", letter as char))?;
    volume.sync_all()?;
    let control = |code| {
        let mut bytes = 0;
        let success = unsafe { DeviceIoControl(volume.as_raw_handle(), code, std::ptr::null(), 0, std::ptr::null_mut(), 0, &mut bytes, std::ptr::null_mut()) };
        if success == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    };
    control(FSCTL_LOCK_VOLUME)?;
    if let Err(error) = control(FSCTL_DISMOUNT_VOLUME) {
        let _ = control(FSCTL_UNLOCK_VOLUME);
        return Err(error);
    }
    Ok(if control(IOCTL_STORAGE_EJECT_MEDIA).is_ok() { EjectOutcome::Ejected } else { EjectOutcome::Unmounted })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn eject_mounted_device(_: &Path) -> io::Result<EjectOutcome> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "Automatic Kobo eject is unavailable on this platform"))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    fn output(success: bool, stdout: &str) -> Output {
        Output { status: ExitStatus::from_raw(if success { 0 } else { 256 }), stdout: stdout.as_bytes().to_vec(), stderr: b"test failure".to_vec() }
    }
    #[test]
    fn flushes_and_unmounts_before_power_off_without_shell_interpolation() {
        let root = Path::new("/media/Kobo with spaces");
        let mut calls = vec![];
        let result = eject_linux(root, &mut |program, args| {
            calls.push((program.to_owned(), args.to_vec()));
            Ok(output(true, if program == "findmnt" { "/dev/sda1\n" } else { "" }))
        })
        .unwrap();
        assert_eq!(result, EjectOutcome::Ejected);
        assert_eq!(calls.iter().map(|(program, _)| program.as_str()).collect::<Vec<_>>(), ["findmnt", "sync", "udisksctl", "udisksctl"]);
        assert_eq!(calls[0].1.last().unwrap(), root.as_os_str());
        assert_eq!(calls[2].1[0], "unmount");
        assert_eq!(calls[3].1[0], "power-off");
    }
    #[test]
    fn failed_unmount_never_attempts_power_off() {
        let mut calls = vec![];
        assert!(eject_linux(Path::new("/media/Kobo"), &mut |program, args| {
            calls.push(args[0].clone());
            Ok(output(program != "udisksctl", if program == "findmnt" { "/dev/sda\n" } else { "" }))
        })
        .is_err());
        assert_eq!(calls.last().unwrap(), "unmount");
    }
    #[test]
    fn power_off_failure_reports_safe_unmount() {
        let result = eject_linux(Path::new("/media/Kobo"), &mut |program, args| Ok(output(args[0] != "power-off", if program == "findmnt" { "/dev/sda\n" } else { "" }))).unwrap();
        assert_eq!(result, EjectOutcome::Unmounted);
    }
    #[test]
    fn a_test_directory_cannot_eject_its_parent_filesystem() {
        let mut calls = 0;
        assert!(eject_linux(Path::new("/tmp/Kobo"), &mut |_, _| {
            calls += 1;
            Ok(output(false, ""))
        })
        .is_err());
        assert_eq!(calls, 1);
    }
}
