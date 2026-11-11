//! Android's in-app browser. Each session owns its download directory until
//! the WebView closes and all of its transfers have finished.
use client_protocol::{BackendError, Failure, OperationError};
use super::{BrowserEvent, BrowserFeedback, BrowserRequest};
use serde::Deserialize;
use serde_json::{from_str, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    Arc, LazyLock, Mutex, OnceLock, PoisonError,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

type Transport = dyn Fn(&str) -> Result<(), OperationError> + Send + Sync;
static TRANSPORT: OnceLock<Arc<Transport>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static SESSIONS: LazyLock<Mutex<HashMap<u64, UnboundedSender<Event>>>> = LazyLock::new(Mutex::default);

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Event {
    Opened,
    Downloaded { path: PathBuf, download: String },
    CancelDownload { download: String },
    Error { message: String, #[serde(default)] failure: Option<Failure> },
    Closed,
}

pub fn initialize(transport: impl Fn(&str) -> Result<(), OperationError> + Send + Sync + 'static) {
    let _ = TRANSPORT.set(Arc::new(transport));
}

pub fn available() -> bool {
    TRANSPORT.get().is_some()
}

/// Only JNI calls this; downloaded paths are checked against the session's
/// staging folder before they enter the library import flow.
pub fn receive(id: u64, message: &str) {
    let Ok(event) = from_str(message) else { return };
    let sender = SESSIONS.lock().unwrap_or_else(PoisonError::into_inner).get(&id).cloned();
    if let Some(sender) = sender {
        let _ = sender.send(event);
    }
}

pub struct BrowserWindow {
    id: u64,
    downloads: PathBuf,
    events: UnboundedReceiver<Event>,
}

#[derive(Clone)]
pub struct BrowserFeedbackSender(u64);

impl BrowserFeedbackSender {
    pub async fn send(&self, feedback: BrowserFeedback) -> Result<(), BackendError> {
        TRANSPORT.get().ok_or("Android browser unavailable")?(&json!({ "command": "feedback", "id": self.0, "feedback": feedback }).to_string()).map_err(Into::into)
    }
}

impl BrowserWindow {
    pub fn feedback_sender(&self) -> BrowserFeedbackSender {
        BrowserFeedbackSender(self.id)
    }
    pub async fn next_event(&mut self) -> Option<BrowserEvent> {
        while let Some(event) = self.events.recv().await {
            match event {
                Event::Downloaded { path, download } => {
                    if let Some(path) = self.accepted_path(path) {
                        return Some(BrowserEvent::Downloaded { path, download });
                    }
                }
                Event::CancelDownload { download } => return Some(BrowserEvent::CancelDownload { download }),
                Event::Closed => return None,
                Event::Opened | Event::Error { .. } => {}
            }
        }
        None
    }

    fn accepted_path(&self, path: PathBuf) -> Option<PathBuf> {
        let path = path.canonicalize().ok()?;
        (path.parent() == Some(self.downloads.as_path()) && path.is_file()).then_some(path)
    }
}

impl Drop for BrowserWindow {
    fn drop(&mut self) {
        SESSIONS.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.id);
        if let Some(transport) = TRANSPORT.get() {
            let _ = transport(&json!({ "command": "close", "id": self.id }).to_string());
        }
    }
}

pub(super) async fn open(request: BrowserRequest) -> Result<BrowserWindow, BackendError> {
    let transport = TRANSPORT.get().ok_or("the Android browser is unavailable")?;
    let downloads = request.downloads.canonicalize().map_err(BackendError::operation)?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let (sender, events) = unbounded_channel();
    SESSIONS.lock().unwrap_or_else(PoisonError::into_inner).insert(id, sender);
    let mut window = BrowserWindow { id, downloads, events };
    transport(&json!({ "command": "open", "id": id, "request": request }).to_string())?;
    match tokio::time::timeout(Duration::from_secs(30), window.events.recv()).await {
        Ok(Some(Event::Opened)) => Ok(window),
        Ok(Some(Event::Error { message, failure })) => Err(failure.map_or_else(|| BackendError::message(message.clone()), |failure| BackendError::classified(failure, message.clone()))),
        _ => Err("the Android browser did not open".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::fs;

    #[tokio::test]
    async fn transport_opens_sessions_and_propagates_startup_errors() {
        initialize(|message| {
            let message: Value = from_str(message).unwrap();
            let id = message["id"].as_u64().unwrap();
            if message["command"] == "open" {
                if message["request"]["title"] == "Fail" {
                    receive(id, r#"{"kind":"error","message":"WebView unavailable"}"#);
                } else {
                    receive(id, r#"{"kind":"opened"}"#);
                }
            }
            Ok(())
        });
        assert!(available());
        let directory = tempfile::tempdir().unwrap();
        let mut request = BrowserRequest {
            url: "https://example.com".into(),
            title: "Books".into(),
            data_directory: directory.path().to_owned(),
            downloads: directory.path().to_owned(),
            history_file: directory.path().join("history.json"),
            preferences: Default::default(),
        };
        let window = open(request.clone()).await.unwrap();
        let id = window.id;
        assert!(SESSIONS.lock().unwrap().contains_key(&id));
        drop(window);
        assert!(!SESSIONS.lock().unwrap().contains_key(&id));
        request.title = "Fail".into();
        assert_eq!(open(request).await.err().unwrap(), "WebView unavailable");
    }

    #[tokio::test]
    async fn sessions_only_import_completed_files_from_their_own_directory() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("book.epub");
        fs::write(&path, b"book").unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let (sender, events) = unbounded_channel();
        let mut window = BrowserWindow { id: 0, downloads: directory.path().canonicalize().unwrap(), events };
        sender.send(Event::Downloaded { path: outside.path().to_owned(), download: "rejected-download".into() }).unwrap();
        sender.send(Event::Downloaded { path: directory.path().join("missing.epub"), download: "rejected-download".into() }).unwrap();
        sender.send(Event::Downloaded { path: path.clone(), download: "https://example.com/book".into() }).unwrap();
        sender.send(Event::Closed).unwrap();
        assert!(matches!(window.next_event().await, Some(BrowserEvent::Downloaded { path: found, download: id }) if found == path.canonicalize().unwrap() && id == "https://example.com/book"));
        assert!(window.next_event().await.is_none());
    }

    #[tokio::test]
    async fn stale_events_do_not_reach_another_library_session() {
        let (sender, mut events) = unbounded_channel();
        SESSIONS.lock().unwrap().insert(u64::MAX, sender);
        receive(u64::MAX - 1, r#"{"kind":"closed"}"#);
        assert!(events.try_recv().is_err());
        receive(u64::MAX, r#"{"kind":"closed"}"#);
        assert!(matches!(events.recv().await, Some(Event::Closed)));
        SESSIONS.lock().unwrap().remove(&u64::MAX);
    }
}
