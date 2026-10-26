//! Filesystem durability mechanisms independent of book layout.
use std::io::{self, Error, ErrorKind};
use std::path::Path;
use std::time::Duration;

#[derive(Default)]
struct Retry {
    attempts: usize,
}

impl Retry {
    fn wait(&mut self, error: &Error) -> bool {
        let transient = matches!(error.kind(), ErrorKind::Interrupted | ErrorKind::WouldBlock | ErrorKind::TimedOut);
        #[cfg(windows)]
        let transient = transient || matches!(error.raw_os_error(), Some(32 | 33)); // Sharing/lock violation.
        const DELAYS: [Duration; 3] = [Duration::from_millis(10), Duration::from_millis(40), Duration::from_millis(100)];
        if !transient || self.attempts == DELAYS.len() {
            return false;
        }
        std::thread::sleep(DELAYS[self.attempts]);
        self.attempts += 1;
        true
    }
}

/// Retry a synchronous filesystem operation after a transient failure, with a
/// bounded delay. The operation must be safe to repeat after returning an error;
/// do not use this to restart writes that may have made partial progress.
pub fn retry_transient<T>(mut operation: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut retry = Retry::default();
    loop {
        match operation() {
            Err(error) if retry.wait(&error) => {}
            result => return result,
        }
    }
}

pub fn parent_directory(path: &Path) -> io::Result<&Path> {
    path.parent().ok_or_else(|| Error::new(ErrorKind::InvalidInput, format!("path has no parent directory: {}", path.display())))
}

pub fn sync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    retry_transient(|| std::fs::File::open(path)?.sync_all())?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
pub fn persist_noclobber(temporary: tempfile::TempPath, destination: &Path) -> Result<(), tempfile::PathPersistError> {
    persist_with_retry(temporary, |temporary| temporary.persist_noclobber(destination))
}

fn persist_with_retry(mut temporary: tempfile::TempPath, mut persist: impl FnMut(tempfile::TempPath) -> Result<(), tempfile::PathPersistError>) -> Result<(), tempfile::PathPersistError> {
    let mut retry = Retry::default();
    loop {
        match persist(temporary) {
            Err(error) if retry.wait(&error.error) => temporary = error.path,
            result => return result,
        }
    }
}
/// Rename a file or directory without replacing an existing destination.
pub fn rename_noclobber(source: &Path, destination: &Path) -> io::Result<()> {
    retry_transient(|| rename_noclobber_once(source, destination))
}

fn rename_noclobber_once(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt as _;
        let source = CString::new(source.as_os_str().as_bytes())?;
        let destination = CString::new(destination.as_os_str().as_bytes())?;
        // Both C strings remain alive for the syscall, which only reads them.
        #[cfg(any(target_os = "linux", target_os = "android"))]
        let result = unsafe { libc::syscall(libc::SYS_renameat2, libc::AT_FDCWD, source.as_ptr(), libc::AT_FDCWD, destination.as_ptr(), libc::RENAME_NOREPLACE) };
        #[cfg(target_os = "macos")]
        let result = unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
        if result == 0 {
            Ok(())
        } else {
            Err(Error::last_os_error())
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn MoveFileExW(source: *const u16, destination: *const u16, flags: u32) -> i32;
        }
        let wide = |path: &Path| -> io::Result<Vec<u16>> {
            let mut path: Vec<_> = path.as_os_str().encode_wide().collect();
            if path.contains(&0) {
                return Err(Error::new(ErrorKind::InvalidInput, "path contains a NUL character"));
            }
            path.push(0);
            Ok(path)
        };
        let source = wide(source)?;
        let destination = wide(destination)?;
        // With no replacement flag, Windows rejects an existing destination
        // atomically. Both terminated strings remain alive for the call.
        if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) } != 0 {
            Ok(())
        } else {
            Err(Error::last_os_error())
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", windows)))]
    {
        let _ = (source, destination);
        Err(Error::new(ErrorKind::Unsupported, "exclusive rename is unavailable on this platform"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn transient_failures_recover_without_restarting_the_caller() {
        let mut failures = [ErrorKind::Interrupted, ErrorKind::WouldBlock, ErrorKind::TimedOut].into_iter();
        let result = retry_transient(|| match failures.next() {
            Some(kind) => Err(io::Error::from(kind)),
            None => Ok(42),
        });
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn persistent_transient_failure_stops_after_four_attempts() {
        let mut attempts = 0;
        let result = retry_transient(|| {
            attempts += 1;
            Err::<(), _>(io::Error::from(ErrorKind::WouldBlock))
        });
        assert_eq!(result.unwrap_err().kind(), ErrorKind::WouldBlock);
        assert_eq!(attempts, 4);
    }

    #[test]
    fn permanent_errors_are_returned_without_retrying() {
        for kind in [ErrorKind::PermissionDenied, ErrorKind::StorageFull, ErrorKind::NotFound, ErrorKind::AlreadyExists, ErrorKind::Unsupported, ErrorKind::InvalidInput] {
            let mut attempts = 0;
            let result = retry_transient(|| {
                attempts += 1;
                Err::<(), _>(io::Error::from(kind))
            });
            assert_eq!(result.unwrap_err().kind(), kind);
            assert_eq!(attempts, 1);
        }
    }

    #[test]
    fn transient_publication_failure_reuses_the_complete_staged_file() {
        let root = tempfile::tempdir().unwrap();
        let temporary = tempfile::NamedTempFile::new_in(root.path()).unwrap().into_temp_path();
        fs::write(&temporary, b"verified book").unwrap();
        let destination = root.path().join("book.epub");
        let mut attempts = 0;
        persist_with_retry(temporary, |temporary| {
            attempts += 1;
            if attempts == 1 {
                Err(tempfile::PathPersistError { error: ErrorKind::WouldBlock.into(), path: temporary })
            } else {
                temporary.persist_noclobber(&destination)
            }
        })
        .unwrap();
        assert_eq!(attempts, 2);
        assert_eq!(fs::read(destination).unwrap(), b"verified book");
    }

    #[test]
    fn exhausted_publication_retries_return_the_staged_file_for_recovery() {
        let root = tempfile::tempdir().unwrap();
        let temporary = tempfile::NamedTempFile::new_in(root.path()).unwrap().into_temp_path();
        fs::write(&temporary, b"verified book").unwrap();
        let mut attempts = 0;
        let error = persist_with_retry(temporary, |temporary| {
            attempts += 1;
            Err(tempfile::PathPersistError { error: ErrorKind::WouldBlock.into(), path: temporary })
        })
        .unwrap_err();
        assert_eq!(attempts, 4);
        assert_eq!(error.error.kind(), ErrorKind::WouldBlock);
        assert_eq!(fs::read(&error.path).unwrap(), b"verified book");
        let destination = root.path().join("book.epub");
        persist_noclobber(error.path, &destination).unwrap();
        assert_eq!(fs::read(destination).unwrap(), b"verified book");
    }

    #[cfg(windows)]
    #[test]
    fn publication_recovers_when_a_windows_sharing_lock_is_released() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        let temporary = tempfile::NamedTempFile::new_in(root.path()).unwrap().into_temp_path();
        fs::write(&temporary, b"book").unwrap();
        let locked = fs::OpenOptions::new().read(true).share_mode(0).open(&temporary).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            drop(locked);
        });
        let destination = root.path().join("book.epub");
        let result = persist_noclobber(temporary, &destination);
        release.join().unwrap();
        result.unwrap();
        assert_eq!(fs::read(destination).unwrap(), b"book");
    }

    #[test]
    fn rename_noclobber_moves_files_and_directories_to_absent_destinations() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("source-file");
        let moved_file = root.path().join("moved-file");
        fs::write(&file, b"book").unwrap();
        rename_noclobber(&file, &moved_file).unwrap();
        assert!(!file.exists());
        assert_eq!(fs::read(moved_file).unwrap(), b"book");
        let directory = root.path().join("source-directory");
        let moved_directory = root.path().join("moved-directory");
        fs::create_dir(&directory).unwrap();
        fs::write(directory.join("book"), b"nested book").unwrap();
        rename_noclobber(&directory, &moved_directory).unwrap();
        assert!(!directory.exists());
        assert_eq!(fs::read(moved_directory.join("book")).unwrap(), b"nested book");
    }

    #[test]
    fn rename_noclobber_keeps_both_existing_files() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source");
        let destination = directory.path().join("destination");
        std::fs::write(&source, b"new book").unwrap();
        std::fs::write(&destination, b"existing book").unwrap();
        assert!(rename_noclobber(&source, &destination).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), b"new book");
        assert_eq!(std::fs::read(&destination).unwrap(), b"existing book");
    }

    #[test]
    fn persist_keeps_existing_destination_and_recoverable_temporary() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("existing");
        std::fs::write(&target, b"original").unwrap();
        let temporary = tempfile::NamedTempFile::new_in(root.path()).unwrap().into_temp_path();
        std::fs::write(&temporary, b"replacement").unwrap();
        let error = persist_noclobber(temporary, &target).unwrap_err();
        assert_eq!(std::fs::read(&target).unwrap(), b"original");
        assert_eq!(std::fs::read(&error.path).unwrap(), b"replacement");
        let next = root.path().join("new");
        persist_noclobber(error.path, &next).unwrap();
        sync_directory(root.path()).unwrap();
        assert_eq!(std::fs::read(next).unwrap(), b"replacement");
    }
}

/// An owned source opened by the backend thread when its import reaches the file.
pub struct ImportSource(Box<dyn FnOnce() -> ImportReaderFuture + Send>, Option<u64>);
pub type ImportReader = Box<dyn std::io::Read + Send>;
pub type ImportReaderFuture = std::pin::Pin<Box<dyn std::future::Future<Output = Result<ImportReader, String>> + Send>>;
impl ImportSource {
    /// The opener must not depend on the UI thread.
    pub fn new(open: Box<dyn FnOnce() -> ImportReaderFuture + Send>) -> Self {
        Self(open, None)
    }
    pub fn with_size_bytes(mut self, size: Option<u64>) -> Self {
        self.1 = size;
        self
    }
    pub fn size_bytes(&self) -> Option<u64> {
        self.1
    }
    pub async fn open(self) -> Result<ImportReader, String> {
        (self.0)().await
    }
}
