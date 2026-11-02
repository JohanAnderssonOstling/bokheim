use std::io;
use std::path::{Path, PathBuf};

use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions};

pub(super) type Endpoint = PathBuf;
pub(super) type Client = NamedPipeClient;

pub(super) fn endpoint(data: &Path) -> io::Result<Endpoint> {
    Ok(PathBuf::from(format!(r"\\.\pipe\Bokheim-reader-{:016x}", super::endpoint_id(data)?)))
}

pub(super) struct Listener {
    inner: NamedPipeServer,
    endpoint: Endpoint,
}

impl Listener {
    pub(super) fn bind(endpoint: &Endpoint) -> io::Result<Self> {
        let inner = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).create(endpoint)?;
        Ok(Self { inner, endpoint: endpoint.clone() })
    }

    pub(super) async fn accept(&mut self) -> io::Result<NamedPipeServer> {
        self.inner.connect().await?;
        // Keep a listening instance available while the connected one is handled.
        let next = ServerOptions::new().reject_remote_clients(true).create(&self.endpoint)?;
        Ok(std::mem::replace(&mut self.inner, next))
    }
}

pub(super) async fn connect(endpoint: &Endpoint) -> io::Result<NamedPipeClient> {
    ClientOptions::new().open(endpoint)
}

pub(super) fn connection_pending(error: &io::Error) -> bool {
    // FILE_NOT_FOUND, PIPE_BUSY, NO_DATA, PIPE_NOT_CONNECTED during replacement.
    matches!(error.raw_os_error(), Some(2 | 231 | 232 | 233)) || error.kind() == io::ErrorKind::Interrupted
}
