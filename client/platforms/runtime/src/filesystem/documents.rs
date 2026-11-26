use serde_json::{Value, json};
use std::ffi::OsString;
pub use std::fs::{File, Permissions};
use std::io::{self, ErrorKind, Read, Write};
use std::os::fd::FromRawFd;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

type Bridge = Box<dyn Fn(&str) -> io::Result<String> + Send + Sync>;
static BRIDGE: OnceLock<Bridge> = OnceLock::new();

/// Retain the first application-context bridge across Activity recreation.
pub fn configure_documents(bridge: impl Fn(&str) -> io::Result<String> + Send + Sync + 'static) {
    BRIDGE.get_or_init(|| Box::new(bridge));
}
pub fn is_document_path(path: &Path) -> bool {
    path.starts_with("/bokheim-document-trees")
}
pub(crate) fn command(request: Value) -> io::Result<Value> {
    let bridge = BRIDGE.get().ok_or_else(|| io::Error::other("document storage is not initialized"))?;
    let response: Value = serde_json::from_str(&bridge(&request.to_string())?).map_err(io::Error::other)?;
    if let Some(message) = response.get("error").and_then(Value::as_str) {
        let kind = match response.get("kind").and_then(Value::as_str) {
            Some("missing") => ErrorKind::NotFound,
            Some("exists") => ErrorKind::AlreadyExists,
            Some("permission") => ErrorKind::PermissionDenied,
            Some("unsupported") => ErrorKind::Unsupported,
            Some("crosses") => ErrorKind::CrossesDevices,
            _ => ErrorKind::Other,
        };
        return Err(io::Error::new(kind, message.to_owned()));
    }
    Ok(response["value"].clone())
}

#[derive(Clone, Debug)]
pub enum Metadata {
    Native(std::fs::Metadata),
    Document { directory: bool, length: u64, modified: u64, identity: String },
}
impl Metadata {
    pub fn is_dir(&self) -> bool {
        self.file_type().is_dir()
    }
    pub fn is_file(&self) -> bool {
        self.file_type().is_file()
    }
    pub fn is_symlink(&self) -> bool {
        self.file_type().is_symlink()
    }
    pub fn len(&self) -> u64 {
        match self {
            Self::Native(value) => value.len(),
            Self::Document { length, .. } => *length,
        }
    }
    pub fn modified(&self) -> io::Result<SystemTime> {
        match self {
            Self::Native(value) => value.modified(),
            Self::Document { modified, .. } => Ok(UNIX_EPOCH + Duration::from_millis(*modified)),
        }
    }
    pub fn file_type(&self) -> FileType {
        match self {
            Self::Native(value) => FileType::Native(value.file_type()),
            Self::Document { directory, .. } => FileType::Document(*directory),
        }
    }
    pub fn permissions(&self) -> Permissions {
        match self {
            Self::Native(value) => value.permissions(),
            Self::Document { .. } => {
                use std::os::unix::fs::PermissionsExt;
                Permissions::from_mode(0o600)
            }
        }
    }
    pub fn document_identity(&self) -> Option<&str> {
        match self {
            Self::Document { identity, .. } => Some(identity),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum FileType {
    Native(std::fs::FileType),
    Document(bool),
}
impl FileType {
    pub fn is_dir(self) -> bool {
        match self {
            Self::Native(value) => value.is_dir(),
            Self::Document(directory) => directory,
        }
    }
    pub fn is_file(self) -> bool {
        match self {
            Self::Native(value) => value.is_file(),
            Self::Document(directory) => !directory,
        }
    }
    pub fn is_symlink(self) -> bool {
        match self {
            Self::Native(value) => value.is_symlink(),
            Self::Document(_) => false,
        }
    }
}
fn document_metadata(value: &Value) -> Metadata {
    Metadata::Document {
        directory: value["directory"].as_bool().unwrap_or(false),
        length: value["size"].as_u64().unwrap_or(0),
        modified: value["modified"].as_u64().unwrap_or(0),
        identity: value["id"].as_str().unwrap_or_default().to_owned(),
    }
}
pub fn metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
    metadata_path(path.as_ref())
}
fn metadata_path(path: &Path) -> io::Result<Metadata> {
    if is_document_path(path) { Ok(document_metadata(&command(json!({"op": "stat", "path": path}))?)) } else { std::fs::metadata(path).map(Metadata::Native) }
}
pub fn symlink_metadata(path: impl AsRef<Path>) -> io::Result<Metadata> {
    let path = path.as_ref();
    if is_document_path(path) { metadata(path) } else { std::fs::symlink_metadata(path).map(Metadata::Native) }
}
pub fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    canonicalize_path(path.as_ref())
}
fn canonicalize_path(path: &Path) -> io::Result<PathBuf> {
    if !is_document_path(path) {
        return std::fs::canonicalize(path);
    }
    if path.components().any(|component| matches!(component, Component::ParentDir | Component::CurDir)) {
        return Err(io::Error::new(ErrorKind::InvalidInput, "invalid document path"));
    }
    metadata(path)?;
    Ok(path.to_path_buf())
}
pub fn open(path: impl AsRef<Path>) -> io::Result<File> {
    OpenOptions::new().read(true).open(path)
}
pub fn create(path: impl AsRef<Path>) -> io::Result<File> {
    OpenOptions::new().write(true).create(true).truncate(true).open(path)
}
#[derive(Default)]
pub struct OpenOptions {
    read: bool,
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
}
impl OpenOptions {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn read(&mut self, value: bool) -> &mut Self {
        self.read = value;
        self
    }
    pub fn write(&mut self, value: bool) -> &mut Self {
        self.write = value;
        self
    }
    pub fn append(&mut self, value: bool) -> &mut Self {
        self.append = value;
        self
    }
    pub fn truncate(&mut self, value: bool) -> &mut Self {
        self.truncate = value;
        self
    }
    pub fn create(&mut self, value: bool) -> &mut Self {
        self.create = value;
        self
    }
    pub fn create_new(&mut self, value: bool) -> &mut Self {
        self.create_new = value;
        self
    }
    pub fn open(&self, path: impl AsRef<Path>) -> io::Result<File> {
        self.open_path(path.as_ref())
    }
    fn open_path(&self, path: &Path) -> io::Result<File> {
        if !is_document_path(path) {
            return std::fs::OpenOptions::new().read(self.read).write(self.write).append(self.append).truncate(self.truncate).create(self.create).create_new(self.create_new).open(path);
        }
        let value = command(json!({"op": "open", "path": path, "read": self.read, "write": self.write, "append": self.append, "truncate": self.truncate, "create": self.create, "exclusive": self.create_new}))?;
        let fd = value.as_i64().and_then(|value| i32::try_from(value).ok()).filter(|fd| *fd >= 0).ok_or_else(|| io::Error::other("document provider returned no file descriptor"))?;
        // The Java bridge detached this descriptor; ownership transfers once.
        Ok(unsafe { File::from_raw_fd(fd) })
    }
}
pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    open(path)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}
pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    let mut text = String::new();
    open(path)?.read_to_string(&mut text)?;
    Ok(text)
}
pub fn write(path: impl AsRef<Path>, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    create(path)?.write_all(bytes.as_ref())
}
pub fn create_dir(path: impl AsRef<Path>) -> io::Result<()> {
    create_dir_path(path.as_ref())
}
fn create_dir_path(path: &Path) -> io::Result<()> {
    if is_document_path(path) { command(json!({"op": "mkdir", "path": path})).map(|_| ()) } else { std::fs::create_dir(path) }
}
pub fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    create_dir_all_path(path.as_ref())
}
fn create_dir_all_path(path: &Path) -> io::Result<()> {
    if !is_document_path(path) {
        return std::fs::create_dir_all(path);
    }
    match metadata(path) {
        Ok(value) if value.is_dir() => return Ok(()),
        Ok(_) => return Err(io::Error::new(ErrorKind::AlreadyExists, "document is not a directory")),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    create_dir_all_path(path.parent().ok_or_else(|| io::Error::other("document path has no parent"))?)?;
    match create_dir(path) {
        Err(error) if error.kind() == ErrorKind::AlreadyExists && super::is_dir(path) => Ok(()),
        result => result,
    }
}
fn remove(path: &Path, directory: bool, recursive: bool) -> io::Result<()> {
    if is_document_path(path) {
        command(json!({"op": "remove", "path": path, "directory": directory, "recursive": recursive})).map(|_| ())
    } else if !directory {
        std::fs::remove_file(path)
    } else if recursive {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_dir(path)
    }
}
pub fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
    remove(path.as_ref(), false, false)
}
pub fn remove_dir(path: impl AsRef<Path>) -> io::Result<()> {
    remove(path.as_ref(), true, false)
}
pub fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    remove(path.as_ref(), true, true)
}
pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    move_path(from.as_ref(), to.as_ref(), false)
}
pub fn rename_noclobber(from: &Path, to: &Path) -> io::Result<()> {
    move_path(from, to, true)
}
pub fn hard_link(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    let (from, to) = (from.as_ref(), to.as_ref());
    if is_document_path(from) || is_document_path(to) {
        return Err(io::Error::new(ErrorKind::Unsupported, "document storage does not support hard links"));
    }
    std::fs::hard_link(from, to)
}
fn move_path(from: &Path, to: &Path, exclusive: bool) -> io::Result<()> {
    let (from, to) = (from, to);
    if is_document_path(from) != is_document_path(to) {
        return Err(io::ErrorKind::CrossesDevices.into());
    }
    if is_document_path(from) || is_document_path(to) {
        command(json!({"op": "rename", "path": from, "to": to, "exclusive": exclusive})).map(|_| ())
    } else if exclusive {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let from = CString::new(from.as_os_str().as_bytes())?;
        let to = CString::new(to.as_os_str().as_bytes())?;
        // Both terminated strings remain alive for this synchronous syscall.
        if unsafe { libc::syscall(libc::SYS_renameat2, libc::AT_FDCWD, from.as_ptr(), libc::AT_FDCWD, to.as_ptr(), libc::RENAME_NOREPLACE) } == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
    } else {
        std::fs::rename(from, to)
    }
}
pub fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
    io::copy(&mut open(from)?, &mut create(to)?)
}
pub fn set_permissions(path: impl AsRef<Path>, permissions: Permissions) -> io::Result<()> {
    if is_document_path(path.as_ref()) { Ok(()) } else { std::fs::set_permissions(path, permissions) }
}
#[derive(Debug)]
pub struct DirEntry {
    path: PathBuf,
    metadata: Metadata,
}
impl DirEntry {
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }
    pub fn file_name(&self) -> OsString {
        self.path.file_name().unwrap_or_default().to_owned()
    }
    pub fn file_type(&self) -> io::Result<FileType> {
        Ok(self.metadata.file_type())
    }
    pub fn metadata(&self) -> io::Result<Metadata> {
        Ok(self.metadata.clone())
    }
}
pub type ReadDir = std::vec::IntoIter<io::Result<DirEntry>>;
pub fn read_dir(path: impl AsRef<Path>) -> io::Result<ReadDir> {
    read_dir_path(path.as_ref())
}
fn read_dir_path(path: &Path) -> io::Result<ReadDir> {
    if !is_document_path(path) {
        return Ok(std::fs::read_dir(path)?
            .map(|entry| {
                let entry = entry?;
                Ok(DirEntry { path: entry.path(), metadata: Metadata::Native(std::fs::symlink_metadata(entry.path())?) })
            })
            .collect::<Vec<_>>()
            .into_iter());
    }
    let value = command(json!({"op": "list", "path": path}))?;
    let children = value.as_array().ok_or_else(|| io::Error::other("invalid document listing"))?;
    children
        .iter()
        .map(|child| {
            let name = child["name"].as_str().ok_or_else(|| io::Error::other("document has no name"))?;
            if name.is_empty() || name == "." || name == ".." || name.contains('/') {
                return Err(io::Error::other("invalid document name"));
            }
            Ok(Ok(DirEntry { path: path.join(name), metadata: document_metadata(child) }))
        })
        .collect::<io::Result<Vec<_>>>()
        .map(Vec::into_iter)
}

impl std::os::unix::fs::MetadataExt for Metadata {
    fn dev(&self) -> u64 {
        match self {
            Self::Native(value) => value.dev(),
            _ => 0,
        }
    }
    fn ino(&self) -> u64 {
        match self {
            Self::Native(value) => value.ino(),
            Self::Document { identity, .. } => {
                use std::hash::{Hash, Hasher};
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                identity.hash(&mut hash);
                hash.finish()
            }
        }
    }
    fn mode(&self) -> u32 {
        match self {
            Self::Native(value) => value.mode(),
            _ => {
                if self.is_dir() {
                    0o040700
                } else {
                    0o100600
                }
            }
        }
    }
    fn nlink(&self) -> u64 {
        match self {
            Self::Native(value) => value.nlink(),
            _ => 1,
        }
    }
    fn uid(&self) -> u32 {
        match self {
            Self::Native(value) => value.uid(),
            _ => 0,
        }
    }
    fn gid(&self) -> u32 {
        match self {
            Self::Native(value) => value.gid(),
            _ => 0,
        }
    }
    fn rdev(&self) -> u64 {
        match self {
            Self::Native(value) => value.rdev(),
            _ => 0,
        }
    }
    fn size(&self) -> u64 {
        self.len()
    }
    fn atime(&self) -> i64 {
        match self {
            Self::Native(value) => value.atime(),
            _ => self.mtime(),
        }
    }
    fn atime_nsec(&self) -> i64 {
        match self {
            Self::Native(value) => value.atime_nsec(),
            _ => self.mtime_nsec(),
        }
    }
    fn mtime(&self) -> i64 {
        match self {
            Self::Native(value) => value.mtime(),
            Self::Document { modified, .. } => (*modified / 1000) as i64,
        }
    }
    fn mtime_nsec(&self) -> i64 {
        match self {
            Self::Native(value) => value.mtime_nsec(),
            Self::Document { modified, .. } => ((*modified % 1000) * 1_000_000) as i64,
        }
    }
    fn ctime(&self) -> i64 {
        match self {
            Self::Native(value) => value.ctime(),
            _ => self.mtime(),
        }
    }
    fn ctime_nsec(&self) -> i64 {
        match self {
            Self::Native(value) => value.ctime_nsec(),
            _ => self.mtime_nsec(),
        }
    }
    fn blksize(&self) -> u64 {
        match self {
            Self::Native(value) => value.blksize(),
            _ => 4096,
        }
    }
    fn blocks(&self) -> u64 {
        match self {
            Self::Native(value) => value.blocks(),
            _ => self.len().div_ceil(512),
        }
    }
}

/// Keep Android's launcher gate and native preferences on the same location.
pub fn set_default_document_root(path: &str) -> io::Result<()> {
    command(json!({"op": "setDefault", "path": path})).map(|_| ())
}

pub fn show_document_folder(path: &Path) -> io::Result<()> {
    command(json!({"op": "show", "path": path})).map(|_| ())
}

/// Retry interrupted replacements when a library opens or reconnects.
pub fn recover_document_tree(path: &Path) -> io::Result<()> {
    if is_document_path(path) {
        command(json!({"op": "recover", "path": path}))?;
    }
    Ok(())
}
