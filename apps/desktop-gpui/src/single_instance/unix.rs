use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};

use tokio::net::{UnixListener, UnixStream};

pub(super) type Client = UnixStream;

pub(super) struct Endpoint {
    path: PathBuf,
    owner: u32,
}

pub(super) fn endpoint(data: &Path) -> io::Result<Endpoint> {
    let owner = fs::metadata(data)?.uid();
    let mut runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    if let Some(app_id) = std::env::var_os("FLATPAK_ID") {
        // Flatpak shares this directory between separate sandbox instances.
        runtime = runtime.join("app").join(app_id);
    }
    let directory = runtime.join(format!("bokheim-ipc-{owner}-{:016x}", super::endpoint_id(data)?));
    Ok(Endpoint { path: directory.join("reader.sock"), owner })
}

fn validate_directory(endpoint: &Endpoint) -> io::Result<()> {
    let directory = endpoint.path.parent().expect("socket has a directory");
    let metadata = fs::symlink_metadata(directory)?;
    if !metadata.is_dir() || metadata.uid() != endpoint.owner || metadata.mode() & 0o077 != 0 {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "desktop socket directory must be private and owned by the application user"));
    }
    Ok(())
}

pub(super) struct Listener {
    inner: UnixListener,
    path: PathBuf,
}

impl Listener {
    pub(super) fn bind(endpoint: &Endpoint) -> io::Result<Self> {
        let directory = endpoint.path.parent().expect("socket has a directory");
        match DirBuilder::new().mode(0o700).create(directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        validate_directory(endpoint)?;
        // Only the desktop lock owner may replace a socket left by a crash.
        match fs::symlink_metadata(&endpoint.path) {
            Ok(metadata) if metadata.file_type().is_socket() => fs::remove_file(&endpoint.path)?,
            Ok(_) => return Err(io::Error::new(io::ErrorKind::AlreadyExists, "desktop socket path is occupied by another file")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        Ok(Self { inner: UnixListener::bind(&endpoint.path)?, path: endpoint.path.clone() })
    }

    pub(super) async fn accept(&mut self) -> io::Result<UnixStream> {
        self.inner.accept().await.map(|(stream, _)| stream)
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        if let Some(directory) = self.path.parent() {
            let _ = fs::remove_dir(directory);
        }
    }
}

pub(super) async fn connect(endpoint: &Endpoint) -> io::Result<UnixStream> {
    validate_directory(endpoint)?;
    UnixStream::connect(&endpoint.path).await
}

pub(super) fn connection_pending(error: &io::Error) -> bool {
    matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused | io::ErrorKind::ConnectionReset | io::ErrorKind::Interrupted)
}
