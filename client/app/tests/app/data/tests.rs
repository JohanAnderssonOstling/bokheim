use super::*;
use account_client::{AccountSession, Session, SessionPersistence};
use client_platform_runtime::storage_queue;
use library_management::account::SessionStorage;
use std::future::{Future, poll_fn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::task::Poll;
use std::time::Duration;

#[derive(Clone, Debug)]
struct ObservedPersistence {
    context: SessionStorage,
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
    let context = SessionStorage::new(BackendContext::initialize(AppDataLocation::native_path(root.path())).unwrap().registry);
    let (started, starting) = mpsc::channel();
    let armed = Arc::new(AtomicBool::new(false));
    let persistence = ObservedPersistence { context: context.clone(), started, armed: armed.clone() };
    let account = AccountSession::load(persistence, "https://example.com".parse().unwrap()).unwrap();
    let changes = account.subscribe();

    let (blocked, entered) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let blocker = storage_queue::run(move || {
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
    let foreground = storage_queue::run(move || foreground_account.current());
    tokio::pin!(foreground);
    poll_fn(|cx| {
        assert!(foreground.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;

    let setter = account.clone();
    let session = Session::try_new("https://example.com".parse().unwrap(), "user".into(), "reader@example.com".into(), "a".repeat(43), i64::MAX - 1, "r".repeat(43), i64::MAX).unwrap();
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
