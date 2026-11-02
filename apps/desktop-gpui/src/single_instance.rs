use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use fs2::FileExt;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::runtime::Builder;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep, timeout};

use crate::ReaderActivation;

#[cfg(unix)]
#[path = "single_instance/unix.rs"]
mod transport;
#[cfg(windows)]
#[path = "single_instance/windows.rs"]
mod transport;

const MAX_ACTIVATION_BYTES: usize = 256 * 1024;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(125);
const MESSAGE_TIMEOUT: Duration = Duration::from_secs(5);
const READY: u8 = 1;
const ACCEPTED: u8 = 2;

#[doc(hidden)]
pub enum DesktopInstance {
    Primary(PrimaryInstance),
    Secondary,
}

#[doc(hidden)]
pub struct PrimaryInstance {
    lock: File,
    start: mpsc::Sender<()>,
    shutdown: Option<oneshot::Sender<()>>,
    listener: Option<JoinHandle<()>>,
    pub activations: async_channel::Receiver<ReaderActivation>,
    pub activation_sender: async_channel::Sender<ReaderActivation>,
}

impl DesktopInstance {
    /// Forward directly to the owning process. Retry only until it is ready;
    /// once the request is sent, an uncertain acknowledgment must not replay it.
    pub fn send_activation(app_data_dir: &Path, activation: &ReaderActivation) -> io::Result<()> {
        let bytes = serde_json::to_vec(activation).map_err(io::Error::other)?;
        if bytes.len() > MAX_ACTIVATION_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "desktop activation is too large"));
        }
        let endpoint = transport::endpoint(app_data_dir)?;
        let runtime = Builder::new_current_thread().enable_all().build()?;
        runtime.block_on(async {
            let mut stream = timeout(STARTUP_TIMEOUT, connect_ready(&endpoint)).await.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "desktop application did not become ready"))??;
            timeout(MESSAGE_TIMEOUT, async {
                stream.write_u32(bytes.len() as u32).await?;
                stream.write_all(&bytes).await?;
                if stream.read_u8().await? != ACCEPTED {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "desktop application did not accept the activation"));
                }
                Ok(())
            })
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "desktop activation was not acknowledged"))?
        })
    }

    pub fn acquire(app_data_dir: &Path) -> io::Result<Self> {
        Self::acquire_with_lock(app_data_dir, FileExt::try_lock_exclusive)
    }

    fn acquire_with_lock(app_data_dir: &Path, try_lock: impl FnOnce(&File) -> io::Result<()>) -> io::Result<Self> {
        let lock = OpenOptions::new().create(true).truncate(false).read(true).write(true).open(app_data_dir.join("desktop-instance.lock"))?;
        match try_lock(&lock) {
            Ok(()) => {}
            Err(error) if error.raw_os_error().is_some_and(|code| Some(code) == fs2::lock_contended_error().raw_os_error()) => return Ok(Self::Secondary),
            Err(error) => return Err(error),
        }

        let runtime = Builder::new_current_thread().enable_all().build()?;
        let endpoint = transport::endpoint(app_data_dir)?;
        let listener = {
            let _runtime = runtime.enter();
            transport::Listener::bind(&endpoint)?
        };
        let (activation_sender, activations) = async_channel::unbounded();
        let forward = activation_sender.clone();
        let (start, mut started) = mpsc::channel(1);
        let (shutdown, mut stopped) = oneshot::channel();
        let listener = thread::Builder::new().name("bokheim-instance-activation".to_owned()).spawn(move || {
            runtime.block_on(async move {
                // A startup that yields to an updater never accepts requests.
                // Connected launchers wait for readiness, then retry on replacement.
                tokio::select! {
                    result = started.recv() => if result.is_none() { return; },
                    _ = &mut stopped => return,
                }
                listen(listener, forward, stopped).await;
            });
        })?;
        Ok(Self::Primary(PrimaryInstance { lock, start, shutdown: Some(shutdown), listener: Some(listener), activations, activation_sender }))
    }
}

async fn connect_ready(endpoint: &transport::Endpoint) -> io::Result<transport::Client> {
    loop {
        match transport::connect(endpoint).await {
            Ok(mut stream) => match stream.read_u8().await {
                Ok(READY) => return Ok(stream),
                Ok(_) => return Err(io::Error::new(io::ErrorKind::InvalidData, "unexpected desktop activation protocol")),
                Err(error) if transport::connection_pending(&error) || matches!(error.kind(), io::ErrorKind::UnexpectedEof | io::ErrorKind::ConnectionAborted | io::ErrorKind::NotConnected | io::ErrorKind::BrokenPipe) => {}
                Err(error) => return Err(error),
            },
            Err(error) if transport::connection_pending(&error) => {}
            Err(error) => return Err(error),
        }
        // A bounded connection retry during startup, not an idle polling loop.
        sleep(Duration::from_millis(50)).await;
    }
}

async fn listen(mut listener: transport::Listener, activations: async_channel::Sender<ReaderActivation>, mut stopped: oneshot::Receiver<()>) {
    loop {
        tokio::select! {
            _ = &mut stopped => break,
            accepted = listener.accept() => match accepted {
                Ok(stream) => {
                    let activations = activations.clone();
                    tokio::spawn(async move {
                        match timeout(MESSAGE_TIMEOUT, receive_activation(stream, &activations)).await {
                            Ok(Ok(())) => {}
                            Ok(Err(error)) => log::warn!("could not receive desktop activation: {error}"),
                            Err(_) => log::warn!("desktop activation connection timed out"),
                        }
                    });
                }
                Err(error) => {
                    log::warn!("could not accept desktop activation: {error}");
                    tokio::select! {
                        _ = &mut stopped => break,
                        _ = sleep(Duration::from_millis(100)) => {}
                    }
                }
            }
        }
    }
}

async fn receive_activation(mut stream: impl AsyncRead + AsyncWrite + Unpin, activations: &async_channel::Sender<ReaderActivation>) -> io::Result<()> {
    stream.write_u8(READY).await?;
    let length = stream.read_u32().await? as usize;
    if length > MAX_ACTIVATION_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "desktop activation is too large"));
    }
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await?;
    let activation = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    activations.send(activation).await.map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "desktop activation receiver is closed"))?;
    stream.write_u8(ACCEPTED).await
}

fn endpoint_id(data: &Path) -> io::Result<u64> {
    // A stable name across app versions, scoped to this app-data location.
    let data = data.canonicalize()?;
    Ok(data.as_os_str().as_encoded_bytes().iter().fold(0xcbf29ce484222325u64, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)))
}

impl PrimaryInstance {
    /// Start accepting only once update handoffs have finished.
    pub fn start_listening(&self) {
        let _ = self.start.try_send(());
    }
}

impl Drop for PrimaryInstance {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(listener) = self.listener.take() {
            let _ = listener.join();
        }
        let _ = FileExt::unlock(&self.lock);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc as blocking;
    use std::time::Instant;

    fn primary(directory: &Path) -> PrimaryInstance {
        let DesktopInstance::Primary(primary) = DesktopInstance::acquire(directory).unwrap() else { panic!("expected primary") };
        primary
    }

    fn receive(primary: &PrimaryInstance) -> ReaderActivation {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(activation) = primary.activations.try_recv() {
                return activation;
            }
            assert!(Instant::now() < deadline, "primary did not receive activation");
            thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn lock_failures_do_not_start_a_listener() {
        for kind in [io::ErrorKind::Unsupported, io::ErrorKind::PermissionDenied, io::ErrorKind::Other] {
            let directory = tempfile::tempdir().unwrap();
            let result = DesktopInstance::acquire_with_lock(directory.path(), |_| Err(io::Error::new(kind, "lock failure")));
            assert!(matches!(result, Err(error) if error.kind() == kind));
            assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn acquiring_ownership_does_not_queue_an_activation() {
        let directory = tempfile::tempdir().unwrap();
        let primary = primary(directory.path());
        assert!(matches!(DesktopInstance::acquire(directory.path()).unwrap(), DesktopInstance::Secondary));
        assert!(primary.activations.try_recv().is_err());
    }

    #[test]
    fn secondary_launch_forwards_links_and_tokens_without_message_files() {
        let directory = tempfile::tempdir().unwrap();
        let primary = primary(directory.path());
        primary.start_listening();
        for urls in [Vec::new(), vec!["bokheim://open?warm".to_owned()]] {
            let activation = ReaderActivation { urls, activation_token: Some("launcher-token".to_owned()) };
            DesktopInstance::send_activation(directory.path(), &activation).unwrap();
            assert_eq!(receive(&primary), activation);
        }
        assert!(primary.activations.try_recv().is_err());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1, "only the ownership lock should be stored");
    }

    #[test]
    fn a_link_waits_until_startup_is_ready() {
        let directory = tempfile::tempdir().unwrap();
        let primary = primary(directory.path());
        let path = directory.path().to_owned();
        let activation: ReaderActivation = vec!["bokheim://open?startup".to_owned()].into();
        let expected = activation.clone();
        let (finished, completion) = blocking::channel();
        let sender = thread::spawn(move || finished.send(DesktopInstance::send_activation(&path, &activation)).unwrap());
        assert!(completion.try_recv().is_err());
        assert!(primary.activations.try_recv().is_err());
        primary.start_listening();
        completion.recv_timeout(Duration::from_secs(3)).unwrap().unwrap();
        assert_eq!(receive(&primary), expected);
        sender.join().unwrap();
    }

    #[test]
    fn a_waiting_launcher_survives_owner_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let first = primary(directory.path());
        let path = directory.path().to_owned();
        let activation: ReaderActivation = vec!["bokheim://open?replacement".to_owned()].into();
        let expected = activation.clone();
        let sender = thread::spawn(move || DesktopInstance::send_activation(&path, &activation));
        drop(first);
        let replacement = primary(directory.path());
        replacement.start_listening();
        sender.join().unwrap().unwrap();
        assert_eq!(receive(&replacement), expected);
        assert!(replacement.activations.try_recv().is_err());
    }

    #[test]
    fn concurrent_launches_each_deliver_once() {
        let directory = tempfile::tempdir().unwrap();
        let primary = primary(directory.path());
        primary.start_listening();
        let senders = (0..16)
            .map(|index| {
                let path = directory.path().to_owned();
                thread::spawn(move || DesktopInstance::send_activation(&path, &vec![format!("bokheim://open?request={index:02}")].into()).unwrap())
            })
            .collect::<Vec<_>>();
        for sender in senders {
            sender.join().unwrap();
        }
        let mut actual = (0..16).map(|_| receive(&primary).urls).collect::<Vec<_>>();
        actual.sort();
        assert_eq!(actual, (0..16).map(|index| vec![format!("bokheim://open?request={index:02}")]).collect::<Vec<_>>());
        assert!(primary.activations.try_recv().is_err());
    }

    #[test]
    fn malformed_connections_do_not_block_following_links() {
        let directory = tempfile::tempdir().unwrap();
        let primary = primary(directory.path());
        primary.start_listening();
        let endpoint = transport::endpoint(directory.path()).unwrap();
        Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let mut stream = connect_ready(&endpoint).await.unwrap();
            stream.write_u32(MAX_ACTIVATION_BYTES as u32 + 1).await.unwrap();
            assert!(stream.read_u8().await.is_err());
        });
        let activation: ReaderActivation = vec!["bokheim://open?valid".to_owned()].into();
        DesktopInstance::send_activation(directory.path(), &activation).unwrap();
        assert_eq!(receive(&primary), activation);
    }
}
