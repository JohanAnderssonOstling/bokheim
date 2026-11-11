//! Native browser windows, history, and download transport.
//! Window hosts render the UI; library clients own downloaded book imports.

#![cfg(not(target_arch = "wasm32"))]
use app_preferences::BrowsingPreferences;
use client_protocol::BackendError;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[cfg(not(target_os = "android"))]
use std::process::Stdio;
#[cfg(not(target_os = "android"))]
use std::sync::Arc;
use std::sync::OnceLock;
#[cfg(not(target_os = "android"))]
use std::time::Duration;
#[cfg(not(target_os = "android"))]
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
    sync::Mutex,
};
use url::Url;

pub mod address;
#[cfg(not(target_os = "android"))]
mod availability;

#[cfg(any(target_os = "android", test))]
pub mod android;
#[cfg(target_os = "android")]
pub use android::{BrowserFeedbackSender, BrowserWindow};

static PROFILE: OnceLock<PathBuf> = OnceLock::new();

pub fn configure(data_directory: PathBuf) {
    let _ = PROFILE.set(data_directory);
}

/// What a browser window opens with.
#[derive(Clone, Serialize, Deserialize)]
pub struct BrowserRequest {
    pub url: String,
    pub title: String,
    /// Shared browser cookies and sign-in, kept between app launches.
    pub data_directory: PathBuf,
    /// Where downloads are saved while they run: a hidden folder the
    /// library's scan skips.
    pub downloads: PathBuf,
    /// Typed addresses are shared by browser windows on this device.
    pub history_file: PathBuf,
    /// Theme and typography selected when the browser was launched.
    pub preferences: BrowsingPreferences,
}

/// One line the browser helper writes: that its window is up, then each book
/// it finished downloading.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserEvent {
    Opened,
    Downloaded { path: PathBuf, download: String },
    CancelDownload { download: String },
}

pub use library_model::BrowserDownloadUpdate as BrowserFeedback;

#[cfg(not(target_os = "android"))]
#[derive(Clone)]
pub struct BrowserFeedbackSender(Arc<Mutex<ChildStdin>>);

#[cfg(not(target_os = "android"))]
impl BrowserFeedbackSender {
    pub async fn send(&self, feedback: BrowserFeedback) -> Result<(), BackendError> {
        let mut bytes = serde_json::to_vec(&feedback).map_err(BackendError::operation)?;
        bytes.push(b'\n');
        let mut input = self.0.lock().await;
        input.write_all(&bytes).await.map_err(BackendError::operation)?;
        input.flush().await.map_err(BackendError::operation)
    }
}

/// An open browser window. The helper reads its stdin until this process
/// ends, so the window closes with the app.
#[cfg(not(target_os = "android"))]
pub struct BrowserWindow {
    _child: Child,
    events: Lines<BufReader<ChildStdout>>,
    feedback: BrowserFeedbackSender,
}

#[cfg(not(target_os = "android"))]
impl BrowserWindow {
    pub fn feedback_sender(&self) -> BrowserFeedbackSender {
        self.feedback.clone()
    }
    pub async fn next_event(&mut self) -> Option<BrowserEvent> {
        loop {
            let line = self.events.next_line().await.ok()??;
            if let Ok(event) = serde_json::from_str(&line) {
                if !matches!(event, BrowserEvent::Opened) {
                    return Some(event);
                }
            }
        }
    }
}

#[cfg(not(target_os = "android"))]
fn helper() -> Result<PathBuf, BackendError> {
    // Released installers and launchers use this executable name and override.
    match std::env::var_os("BOKHEIM_BROWSER_HELPER") {
        Some(path) => Ok(PathBuf::from(path)),
        None => Ok(std::env::current_exe().map_err(BackendError::operation)?.with_file_name(format!("bokheim-browser{}", std::env::consts::EXE_SUFFIX))),
    }
}

/// Whether the embedded browser is available here: the helper can load its runtime
/// (WebKitGTK on Linux, WebView2 on Windows), and Linux has X11/XWayland.
/// Probed once per run; opening an actual window can still fail.
#[cfg(not(target_os = "android"))]
pub fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE
        .get_or_init(|| PROFILE.get().is_some() && (!cfg!(target_os = "linux") || std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty())) && helper().is_ok_and(|helper| availability::probe(&helper, Duration::from_secs(5))))
}

/// The Linux dynamic loader exits with 127 when the system WebKitGTK is absent.
#[cfg(not(target_os = "android"))]
fn webkit_missing(status: std::process::ExitStatus) -> bool {
    cfg!(target_os = "linux") && status.code() == Some(127)
}

/// Opens `url` in a browser window, saving what it downloads
/// into `downloads`.
pub async fn open(url: &Url, title: &str, downloads: &Path, preferences: BrowsingPreferences) -> Result<BrowserWindow, BackendError> {
    let profile = PROFILE.get().ok_or("the embedded browser is unavailable on this device")?;
    let request = BrowserRequest { url: url.to_string(), title: title.to_owned(), data_directory: profile.join("profile"), downloads: downloads.to_owned(), history_file: profile.join("browser-address-history.json"), preferences };
    #[cfg(target_os = "android")]
    return android::open(request).await.map_err(BackendError::from);
    #[cfg(not(target_os = "android"))]
    open_desktop(request).await
}

#[cfg(target_os = "android")]
pub fn available() -> bool {
    PROFILE.get().is_some() && android::available()
}

#[cfg(not(target_os = "android"))]
async fn open_desktop(request: BrowserRequest) -> Result<BrowserWindow, BackendError> {
    let mut input = serde_json::to_vec(&request).map_err(BackendError::operation)?;
    input.push(b'\n');
    // Dropped when the window has closed, or when it failed to open.
    let mut command = Command::from(availability::command(&helper()?));
    // Wry embeds into GPUI's native window on X11, including XWayland.
    #[cfg(target_os = "linux")]
    command.env_remove("WAYLAND_DISPLAY").env("GDK_BACKEND", "x11");
    let mut child = command.arg("browser").stdin(Stdio::piped()).stdout(Stdio::piped()).kill_on_drop(true).spawn().map_err(BackendError::operation)?;
    let stdin = child.stdin.as_mut().ok_or("could not send the browser request to its window")?;
    stdin.write_all(&input).await.map_err(BackendError::operation)?;
    stdin.flush().await.map_err(BackendError::operation)?;
    let feedback = BrowserFeedbackSender(Arc::new(Mutex::new(child.stdin.take().ok_or("browser input closed")?)));
    let mut events = BufReader::new(child.stdout.take().ok_or("could not read from the browser window")?).lines();
    let opened = tokio::time::timeout(Duration::from_secs(30), events.next_line()).await.map_err(|_| "the browser window did not open".to_owned())?.map_err(BackendError::operation)?;
    if opened.as_deref().and_then(|line| serde_json::from_str::<BrowserEvent>(line).ok()).is_some_and(|event| matches!(event, BrowserEvent::Opened)) {
        return Ok(BrowserWindow { _child: child, events, feedback });
    }
    // Its stdout ended without the window opening, so it has exited.
    let status = child.wait().await.map_err(BackendError::operation)?;
    if webkit_missing(status) {
        return Err(BackendError::classified(client_protocol::Failure::WebViewMissing, "the browser helper could not load WebKitGTK 4.1 (libwebkit2gtk-4.1)"));
    }
    Err(format!("the browser window could not be opened ({status})").into())
}
