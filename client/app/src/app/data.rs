pub use library_registry::LibraryEntry;
pub(crate) use library_registry::LibraryRegistry;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};

const ACCOUNT_SESSION_KEY: &str = "account_session";

/// Selects where device-local application state is stored.
///
/// Desktop frontends share `Bokheim`'s conventional data directory. Sandboxed
/// platforms such as Android provide the private directory assigned by the OS.
#[derive(Clone, Debug)]
pub enum AppDataLocation {
    SharedDesktop,
    PlatformLocal(String),
}

impl AppDataLocation {
    pub(crate) fn data_dir(&self) -> Result<PathBuf, String> {
        match self {
            Self::SharedDesktop => client_platform_desktop::data_dir(),
            Self::PlatformLocal(locator) => Ok(locator.into()),
        }
    }

    pub(crate) fn default_library_root(&self) -> Result<std::path::PathBuf, String> {
        match self {
            Self::SharedDesktop => client_platform_desktop::default_library_root(),
            Self::PlatformLocal(locator) => Ok(Path::new(locator).join("libraries")),
        }
    }

    /// Converts a path at a native host boundary. Backend state retains only
    /// the resulting storage locator.
    pub fn native_path(path: impl AsRef<Path>) -> Self {
        Self::PlatformLocal(path.as_ref().to_string_lossy().into_owned())
    }
}

/// Validated startup and account-persistence capability.
///
/// Live application state owns the registry directly after startup; this value
/// exists only while opening that state and as the cloneable persistence
/// capability retained by `AccountSession`.
#[derive(Clone, Debug)]
pub(crate) struct BackendContext {
    pub(crate) registry: LibraryRegistry,
    pub(crate) app_data_dir: PathBuf,
}

impl BackendContext {
    #[cfg(any(target_arch = "wasm32", test))]
    pub fn initialize(location: AppDataLocation) -> Result<Self, String> {
        Self::open(location.data_dir()?, &location.default_library_root()?)
    }

    pub(crate) fn open(app_data_dir: PathBuf, library_root: &Path) -> Result<Self, String> {
        let registry = LibraryRegistry::open_with_library_root(app_data_dir.to_string_lossy(), library_root.to_string_lossy()).map_err(|error| format!("failed to initialize library registry: {error}"))?;
        #[cfg(target_os = "android")]
        client_platform_android::book::configure_lease_root(&app_data_dir).map_err(|error| error.to_string())?;
        Ok(Self { registry, app_data_dir })
    }

    pub(super) fn read_json<T: serde::de::DeserializeOwned>(&self, key: &str) -> std::io::Result<Option<T>> {
        self.registry.read_value(key).map_err(|error| Error::other(error.to_string()))?.map(|bytes| serde_json::from_slice(&bytes).map_err(|error| Error::new(ErrorKind::InvalidData, error))).transpose()
    }

    pub(super) fn write_json(&self, key: &str, value: &impl serde::Serialize) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(value).map_err(|error| Error::new(ErrorKind::InvalidData, error))?;
        self.registry.write_value(key, &bytes).map_err(|error| Error::other(error.to_string()))
    }

    pub(crate) fn load_account_session(&self) -> std::io::Result<Option<account_client::Session>> {
        self.read_json(ACCOUNT_SESSION_KEY)
    }

    pub(crate) fn save_account_session(&self, session: &account_client::Session) -> std::io::Result<()> {
        self.write_json(ACCOUNT_SESSION_KEY, session)
    }

    pub(crate) fn clear_account_session(&self) -> std::io::Result<()> {
        self.registry.remove_value(ACCOUNT_SESSION_KEY).map_err(|error| Error::other(error.to_string()))
    }
}

impl account_client::SessionPersistence for BackendContext {
    fn run<T: Send + 'static>(&self, operation: impl FnOnce() -> T + Send + 'static) -> std::io::Result<T> {
        use client_platform_runtime::storage_queue::{self, Priority};
        storage_queue::run_sync(Priority::Interactive, operation).map_err(Error::other)
    }

    fn load_account_session(&self) -> std::io::Result<Option<account_client::Session>> {
        BackendContext::load_account_session(self)
    }

    fn save_account_session(&self, session: &account_client::Session) -> std::io::Result<()> {
        BackendContext::save_account_session(self, session)
    }

    fn clear_account_session(&self) -> std::io::Result<()> {
        BackendContext::clear_account_session(self)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use account_client::{AccountSession, Session, SessionPersistence};
    use client_platform_runtime::storage_queue::{self, Priority};
    use std::future::{poll_fn, Future};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::task::Poll;
    use std::time::Duration;

    #[derive(Clone, Debug)]
    struct ObservedPersistence {
        context: BackendContext,
        started: mpsc::Sender<()>,
        armed: Arc<AtomicBool>,
    }

    impl ObservedPersistence {
        fn signal(&self) {
            if self.armed.swap(false, Ordering::SeqCst) {
                self.started.send(()).unwrap();
            }
        }
    }

    impl SessionPersistence for ObservedPersistence {
        fn run<T: Send + 'static>(&self, operation: impl FnOnce() -> T + Send + 'static) -> std::io::Result<T> {
            self.signal();
            self.context.run(operation)
        }
        fn load_account_session(&self) -> std::io::Result<Option<Session>> {
            self.context.load_account_session()
        }
        fn save_account_session(&self, session: &Session) -> std::io::Result<()> {
            self.signal();
            self.context.save_account_session(session)
        }
        fn clear_account_session(&self) -> std::io::Result<()> {
            self.context.clear_account_session()
        }
    }

    #[tokio::test]
    async fn queued_session_write_cannot_lock_out_an_earlier_storage_command() {
        let root = tempfile::tempdir().unwrap();
        let context = BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap();
        let (started, starting) = mpsc::channel();
        let armed = Arc::new(AtomicBool::new(false));
        let persistence = ObservedPersistence { context: context.clone(), started, armed: armed.clone() };
        let account = AccountSession::load(persistence, "https://example.com".parse().unwrap()).unwrap();
        let changes = account.subscribe();

        let (blocked, entered) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let blocker = storage_queue::run(Priority::Interactive, move || {
            blocked.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        tokio::pin!(blocker);
        poll_fn(|cx| {
            assert!(blocker.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        entered.recv_timeout(Duration::from_secs(5)).unwrap();

        let foreground_account = account.clone();
        let foreground = storage_queue::run(Priority::Interactive, move || foreground_account.current());
        tokio::pin!(foreground);
        poll_fn(|cx| {
            assert!(foreground.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;

        let setter = account.clone();
        let session = Session::try_new("https://example.com".parse().unwrap(), "user".into(), "reader@example.com".into(), "a".repeat(43)).unwrap();
        let expected_token = session.token().to_owned();
        let (finished, done) = mpsc::channel();
        armed.store(true, Ordering::SeqCst);
        let thread = std::thread::spawn(move || finished.send(setter.set(session)).unwrap());
        // The write has reached its persistence boundary while the earlier
        // command is queued. It must not hold the session lock at this point.
        starting.recv_timeout(Duration::from_secs(5)).unwrap();
        release.send(()).unwrap();
        assert!(tokio::time::timeout(Duration::from_secs(5), foreground).await.unwrap().unwrap().unwrap().is_none());
        done.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
        thread.join().unwrap();
        blocker.await.unwrap();

        assert!(changes.has_changed().unwrap());
        assert_eq!(account.credentials_session().unwrap().token(), expected_token);
        assert_eq!(context.load_account_session().unwrap().unwrap().token(), expected_token);
        account.take().unwrap();
        assert!(context.load_account_session().unwrap().is_none());
    }
}
