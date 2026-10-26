//! The platform's embedded browser, isolated from GPUI: a single browser
//! verification attempt, or a store window that hands back what it downloads.
use app_preferences::BrowsingPreferences;
use client_runtime::BackendError;
#[cfg(not(target_os = "android"))]
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
#[cfg(not(target_os = "android"))]
use std::process::Stdio;
use std::sync::OnceLock;
#[cfg(not(target_os = "android"))]
use std::sync::Arc;
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
pub use android::{StoreFeedbackSender, StoreWindow};

static PROFILE: OnceLock<PathBuf> = OnceLock::new();
#[cfg(not(target_os = "android"))]
static BROWSER: Mutex<()> = Mutex::const_new(());

pub fn configure(data_directory: PathBuf) {
    let _ = PROFILE.set(data_directory);
}

/// All browser entry points reuse the original globe-button profile, including
/// its cookies and website storage from before profiles were shared.
pub fn shared_profile(root: &Path) -> PathBuf {
    root.join("stores").join(blake3::hash(b"null").to_hex().as_str())
}

#[derive(Serialize, Deserialize)]
pub struct Request {
    pub url: String,
    pub authorization: Option<String>,
    pub data_directory: PathBuf,
    #[serde(default)]
    pub navigation_url: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Response {
    Fetched { status: u16, content_type: Option<String>, url: String, body: String },
    Error { message: String },
}

/// What a store window opens with.
#[derive(Clone, Serialize, Deserialize)]
pub struct StoreRequest {
    pub url: String,
    pub title: String,
    /// Shared browser cookies and sign-in, kept between app launches.
    pub data_directory: PathBuf,
    /// Where downloads are saved while they run: a hidden folder the
    /// library's scan skips.
    pub downloads: PathBuf,
    /// Typed addresses are shared by browser windows on this device.
    #[serde(default)]
    pub history_file: Option<PathBuf>,
    /// Theme and typography selected when the browser was launched.
    #[serde(default)]
    pub preferences: BrowsingPreferences,
    #[serde(default)]
    pub transfer_policy_file: Option<PathBuf>,
}

/// One line the store helper writes: that its window is up, then each book
/// it finished downloading.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StoreEvent {
    Opened,
    Downloaded { path: PathBuf, #[serde(default)] download: Option<String> },
    Magnet { url: String },
    CancelDownload { download: String },
    TorrentFile { path: PathBuf, #[serde(default)] download: Option<String> },
}

pub use library_model::BrowserDownloadUpdate as BrowserFeedback;

/// Recognize metadata even when a store gives an opaque URL or filename.
pub fn torrent_download(path: &Path) -> bool {
    if path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("torrent")) { return true; }
    if std::fs::metadata(path).is_ok_and(|metadata| metadata.len() <= 16 * 1024 * 1024) {
        if let Ok(bytes) = std::fs::read(path) {
            return bytes.first() == Some(&b'd') && librqbit_core::torrent_metainfo::torrent_from_bytes(&bytes).is_ok();
        }
    }
    false
}

#[cfg(test)]
mod torrent_tests {
    use super::*;
    use std::fs;

    #[test]
    fn recognizes_torrent_metadata_with_an_opaque_filename() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("download");
        fs::write(&path, b"d4:infod6:lengthi3e4:name8:book.txt12:piece lengthi16384e6:pieces20:12345678901234567890ee").unwrap();
        assert!(torrent_download(&path));
        fs::write(&path, b"ordinary book download").unwrap();
        assert!(!torrent_download(&path));
        fs::write(&path, b"dinvalid metadata").unwrap();
        assert!(!torrent_download(&path));
        assert!(torrent_download(&directory.path().join("Books.TORRENT")));
    }
}

#[cfg(not(target_os = "android"))]
#[derive(Clone)]
pub struct StoreFeedbackSender(Arc<Mutex<ChildStdin>>);

#[cfg(not(target_os = "android"))]
impl StoreFeedbackSender {
    pub async fn send(&self, feedback: BrowserFeedback) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(&feedback).map_err(|error| error.to_string())?;
        bytes.push(b'\n');
        let mut input = self.0.lock().await;
        input.write_all(&bytes).await.map_err(|error| error.to_string())?;
        input.flush().await.map_err(|error| error.to_string())
    }
}

/// An open store window. The helper reads its stdin until this process
/// ends, so the window closes with the app.
#[cfg(not(target_os = "android"))]
pub struct StoreWindow {
    _child: Child,
    events: Lines<BufReader<ChildStdout>>,
    feedback: StoreFeedbackSender,
}

#[cfg(not(target_os = "android"))]
impl StoreWindow {
    pub fn feedback_sender(&self) -> StoreFeedbackSender { self.feedback.clone() }
    pub async fn next_event(&mut self) -> Option<StoreEvent> {
        loop {
            let line = self.events.next_line().await.ok()??;
            if let Ok(event) = serde_json::from_str(&line) {
                if !matches!(event, StoreEvent::Opened) { return Some(event); }
            }
        }
    }

    /// The next book the store downloaded; `None` once its window is closed.
    pub async fn next_download(&mut self) -> Option<PathBuf> {
        loop {
            if let StoreEvent::Downloaded { path, .. } = self.next_event().await? {
                return Some(path);
            }
        }
    }
}

#[cfg(not(target_os = "android"))]
fn helper() -> Result<PathBuf, String> {
    match std::env::var_os("BOKHEIM_OPDS_WEBVIEW_HELPER") {
        Some(path) => Ok(PathBuf::from(path)),
        None => Ok(std::env::current_exe().map_err(|error| error.to_string())?.with_file_name(format!("bokheim-opds-webview{}", std::env::consts::EXE_SUFFIX))),
    }
}

/// Whether stores can be offered here: the helper can load its runtime
/// (WebKitGTK on Linux, WebView2 on Windows), and Linux has X11/XWayland.
/// Probed once per run; opening an actual window can still fail.
#[cfg(not(target_os = "android"))]
pub fn stores_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        PROFILE.get().is_some()
            && (!cfg!(target_os = "linux") || std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty()))
            && helper().is_ok_and(|helper| availability::probe(&helper, Duration::from_secs(5)))
    })
}

/// The Linux dynamic loader exits with 127 when the system WebKitGTK is absent.
#[cfg(not(target_os = "android"))]
fn webkit_missing(status: std::process::ExitStatus) -> bool {
    cfg!(target_os = "linux") && status.code() == Some(127)
}

/// Opens the store at `url` in a window of its own, saving what it downloads
/// into `downloads`.
pub async fn open_store(url: &Url, title: &str, downloads: &Path, preferences: BrowsingPreferences) -> Result<StoreWindow, BackendError> {
    let profile = PROFILE.get().ok_or("stores are unavailable on this device")?;
    let request = StoreRequest { url: url.to_string(), title: title.to_owned(), data_directory: shared_profile(profile), downloads: downloads.to_owned(), history_file: Some(profile.join("browser-address-history.json")), preferences, transfer_policy_file: Some(profile.parent().unwrap_or(profile).join("transfer-policy.json")) };
    #[cfg(target_os = "android")]
    return android::open_store(request).await.map_err(BackendError::from);
    #[cfg(not(target_os = "android"))]
    open_desktop_store(request).await
}

#[cfg(target_os = "android")]
pub fn stores_available() -> bool {
    PROFILE.get().is_some() && android::available()
}

#[cfg(not(target_os = "android"))]
async fn open_desktop_store(request: StoreRequest) -> Result<StoreWindow, BackendError> {
    let mut input = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    input.push(b'\n');
    // Dropped when the window has closed, or when it failed to open.
    let mut command = Command::from(availability::command(&helper()?));
    // Wry embeds into GPUI's native window on X11, including XWayland.
    // Verification still uses Tao's GTK window and supports native Wayland.
    #[cfg(target_os = "linux")]
    command.env_remove("WAYLAND_DISPLAY").env("GDK_BACKEND", "x11");
    let mut child = command.arg("store").stdin(Stdio::piped()).stdout(Stdio::piped()).kill_on_drop(true).spawn().map_err(|error| error.to_string())?;
    let stdin = child.stdin.as_mut().ok_or("could not send the store to its window")?;
    stdin.write_all(&input).await.map_err(|error| error.to_string())?;
    stdin.flush().await.map_err(|error| error.to_string())?;
    let feedback = StoreFeedbackSender(Arc::new(Mutex::new(child.stdin.take().ok_or("browser input closed")?)));
    let mut events = BufReader::new(child.stdout.take().ok_or("could not read from the store window")?).lines();
    let opened = tokio::time::timeout(Duration::from_secs(30), events.next_line()).await.map_err(|_| "the store window did not open".to_owned())?.map_err(|error| error.to_string())?;
    if opened.as_deref().and_then(|line| serde_json::from_str::<StoreEvent>(line).ok()).is_some_and(|event| matches!(event, StoreEvent::Opened)) {
        return Ok(StoreWindow { _child: child, events, feedback });
    }
    // Its stdout ended without the window opening, so it has exited.
    let status = child.wait().await.map_err(|error| error.to_string())?;
    if webkit_missing(status) {
        return Err(BackendError::classified(client_runtime::Failure::WebViewMissing, "the store helper could not load WebKitGTK 4.1 (libwebkit2gtk-4.1)"));
    }
    Err(format!("the store window could not be opened ({status})").into())
}

#[cfg(target_os = "android")]
pub(crate) async fn get(_url: &Url, _authorization: Option<String>, _catalog: &Url) -> Result<(u16, Vec<u8>, Option<String>, Url), String> {
    Err("browser feed verification is unavailable on Android".into())
}

#[cfg(not(target_os = "android"))]
pub(crate) async fn get(url: &Url, authorization: Option<String>, catalog: &Url) -> Result<(u16, Vec<u8>, Option<String>, Url), String> {
    let profile = PROFILE.get().ok_or("browser verification is unavailable on this device")?;
    let executable = helper()?;
    let _browser = BROWSER.lock().await;
    let navigation_url = (authorization.is_none() && catalog.origin() == url.origin()).then(|| catalog.to_string());
    let request = Request { url: url.to_string(), authorization, navigation_url, data_directory: shared_profile(profile) };
    let input = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
    let operation = async {
        let mut child = Command::from(availability::command(&executable)).stdin(Stdio::piped()).stdout(Stdio::piped()).kill_on_drop(true).spawn().map_err(|error| error.to_string())?;
        let mut stdin = child.stdin.take().ok_or("could not send the feed to the verification browser")?;
        stdin.write_all(&input).await.map_err(|error| error.to_string())?;
        drop(stdin);
        let output = child.wait_with_output().await.map_err(|error| error.to_string())?;
        if output.stdout.is_empty() && webkit_missing(output.status) {
            return Err("Browser verification needs WebKitGTK 4.1 (libwebkit2gtk-4.1) installed on this system.".to_owned());
        }
        let response: Response = serde_json::from_slice(&output.stdout).map_err(|_| format!("verification browser exited without a result ({})", output.status))?;
        match response {
            Response::Fetched { status, content_type, url, body } => Ok((status, STANDARD.decode(body).map_err(|error| error.to_string())?, content_type, Url::parse(&url).map_err(|error| error.to_string())?)),
            Response::Error { message } => Err(message),
        }
    };
    tokio::time::timeout(Duration::from_secs(310), operation).await.map_err(|_| "browser verification timed out".to_owned())?
}
