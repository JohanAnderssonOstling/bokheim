//! Temporary ownership works equally for OS files and document-provider files.
use super::{self as fs, File, OpenOptions};
use std::fmt;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
static PRIVATE_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub fn configure_root(root: PathBuf) -> io::Result<()> {
    std::fs::create_dir_all(&root)?;
    match PRIVATE_ROOT.set(root) {
        Ok(()) => Ok(()),
        Err(root) if PRIVATE_ROOT.get() == Some(&root) => Ok(()),
        Err(_) => Err(io::Error::other("temporary storage is already configured with a different root")),
    }
}
fn private_root() -> io::Result<&'static Path> {
    PRIVATE_ROOT.get().map(PathBuf::as_path).ok_or_else(|| io::Error::other("temporary storage is not initialized"))
}
pub fn tempfile() -> io::Result<File> {
    tempfile::tempfile_in(private_root()?)
}
static NEXT: AtomicU64 = AtomicU64::new(0);
fn candidate(parent: &Path, prefix: &str) -> PathBuf {
    let time = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    parent.join(format!("{prefix}{:x}-{time:x}-{:x}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)))
}
#[derive(Debug)]
pub struct TempPath {
    path: PathBuf,
    cleanup: bool,
}
impl TempPath {
    pub fn try_from_path(path: impl Into<PathBuf>) -> io::Result<Self> {
        Ok(Self { path: path.into(), cleanup: true })
    }
    pub fn disable_cleanup(&mut self, disable: bool) {
        self.cleanup = !disable;
    }
    pub fn keep(mut self) -> Result<PathBuf, PathPersistError> {
        self.cleanup = false;
        Ok(self.path.clone())
    }
    pub fn persist(self, path: impl AsRef<Path>) -> Result<(), PathPersistError> {
        self.publish(path.as_ref(), false)
    }
    pub fn persist_noclobber(self, path: impl AsRef<Path>) -> Result<(), PathPersistError> {
        self.publish(path.as_ref(), true)
    }
    fn publish(mut self, destination: &Path, exclusive: bool) -> Result<(), PathPersistError> {
        let result = (|| {
            if exclusive && fs::try_exists(destination)? {
                return Err(io::Error::new(io::ErrorKind::AlreadyExists, "destination exists"));
            }
            if self.path == destination {
                return Ok(());
            }
            match if exclusive { fs::rename_noclobber(&self.path, destination) } else { fs::rename(&self.path, destination) } {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {
                    let mut source = fs::open(&self.path)?;
                    let mut target = OpenOptions::new().write(true).create(true).create_new(exclusive).truncate(true).open(destination)?;
                    let copied = io::copy(&mut source, &mut target).and_then(|_| target.sync_all());
                    if let Err(error) = copied {
                        drop(target);
                        let _ = fs::remove_file(destination);
                        return Err(error);
                    }
                    fs::remove_file(&self.path)
                }
                Err(error) => Err(error),
            }
        })();
        match result {
            Ok(()) => {
                self.cleanup = false;
                Ok(())
            }
            Err(error) => Err(PathPersistError { error, path: self }),
        }
    }
}
impl Deref for TempPath {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}
impl AsRef<Path> for TempPath {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}
impl Drop for TempPath {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_file(&self.path);
        }
    }
}
#[derive(Debug)]
pub struct PathPersistError {
    pub error: io::Error,
    pub path: TempPath,
}
impl fmt::Display for PathPersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for PathPersistError {}
#[derive(Debug)]
pub struct PersistError {
    pub error: io::Error,
    pub file: NamedTempFile,
}
impl fmt::Display for PersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}
impl std::error::Error for PersistError {}
#[derive(Debug)]
pub struct NamedTempFile {
    file: File,
    path: TempPath,
}
impl NamedTempFile {
    pub fn new_in(parent: impl AsRef<Path>) -> io::Result<Self> {
        Builder::new().tempfile_in(parent)
    }
    pub fn from_parts(file: File, path: TempPath) -> Self {
        Self { file, path }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn as_file(&self) -> &File {
        &self.file
    }
    pub fn as_file_mut(&mut self) -> &mut File {
        &mut self.file
    }
    pub fn into_parts(self) -> (File, TempPath) {
        (self.file, self.path)
    }
    pub fn into_temp_path(self) -> TempPath {
        self.path
    }
    pub fn persist(self, destination: impl AsRef<Path>) -> Result<File, PersistError> {
        self.publish(destination.as_ref(), false)
    }
    pub fn persist_noclobber(self, destination: impl AsRef<Path>) -> Result<File, PersistError> {
        self.publish(destination.as_ref(), true)
    }
    fn publish(self, destination: &Path, exclusive: bool) -> Result<File, PersistError> {
        let (file, path) = self.into_parts();
        let result = if exclusive { path.persist_noclobber(destination) } else { path.persist(destination) };
        match result {
            Ok(()) => Ok(file),
            Err(error) => Err(PersistError { error: error.error, file: Self::from_parts(file, error.path) }),
        }
    }
}
impl Read for NamedTempFile {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.file.read(b)
    }
}
impl Write for NamedTempFile {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.file.write(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
impl Seek for NamedTempFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.file.seek(pos)
    }
}
impl Deref for NamedTempFile {
    type Target = File;
    fn deref(&self) -> &File {
        &self.file
    }
}
impl AsRef<Path> for NamedTempFile {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}
#[derive(Debug)]
pub struct TempDir {
    path: PathBuf,
    cleanup: bool,
}
impl TempDir {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn keep(mut self) -> PathBuf {
        self.cleanup = false;
        self.path.clone()
    }
    pub fn close(mut self) -> io::Result<()> {
        fs::remove_dir_all(&self.path)?;
        self.cleanup = false;
        Ok(())
    }
}
impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        if self.cleanup {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
pub struct Builder {
    prefix: String,
}
impl Default for Builder {
    fn default() -> Self {
        Self { prefix: ".bokheim-tmp-".to_owned() }
    }
}
impl Builder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn prefix(&mut self, prefix: impl AsRef<std::ffi::OsStr>) -> &mut Self {
        self.prefix = prefix.as_ref().to_string_lossy().into_owned();
        self
    }
    pub fn tempfile_in(&self, parent: impl AsRef<Path>) -> io::Result<NamedTempFile> {
        for _ in 0..100 {
            let path = candidate(parent.as_ref(), &self.prefix);
            match OpenOptions::new().read(true).write(true).create_new(true).open(&path) {
                Ok(file) => return Ok(NamedTempFile::from_parts(file, TempPath::try_from_path(path)?)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other("could not allocate temporary file"))
    }
    pub fn tempdir_in(&self, parent: impl AsRef<Path>) -> io::Result<TempDir> {
        for _ in 0..100 {
            let path = candidate(parent.as_ref(), &self.prefix);
            match fs::create_dir(&path) {
                Ok(()) => return Ok(TempDir { path, cleanup: true }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::other("could not allocate temporary directory"))
    }
}
