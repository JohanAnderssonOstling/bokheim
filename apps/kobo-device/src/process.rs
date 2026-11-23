//! Firmware subprocess isolation, without depending on a firmware setsid tool.
use std::ffi::OsString;
use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command};

pub(crate) fn kill_group(child: &mut Child) {
    unsafe { libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL); }
    let _ = child.kill();
    let _ = child.wait();
}

/// Replace this helper with a command in its own session/process group. The
/// launcher's watchdog can then terminate the command and all its workers.
pub fn exec_isolated(mut arguments: impl Iterator<Item = OsString>) -> io::Error {
    let Some(program) = arguments.next() else {
        return io::Error::new(io::ErrorKind::InvalidInput, "No firmware command supplied");
    };
    if unsafe { libc::setsid() } < 0 {
        return io::Error::last_os_error();
    }
    Command::new(program).args(arguments).env_remove("LD_LIBRARY_PATH").exec()
}
