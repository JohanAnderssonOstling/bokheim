//! Browser-assisted fetching for pages that require interactive verification.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use url::Url;
#[cfg(not(target_os = "android"))]
use super::{availability, helper, shared_profile, webkit_missing, PROFILE};
#[cfg(not(target_os = "android"))]
use base64::{engine::general_purpose::STANDARD, Engine};
#[cfg(not(target_os = "android"))]
use std::{process::Stdio, time::Duration};
#[cfg(not(target_os = "android"))]
use tokio::{io::AsyncWriteExt, process::Command, sync::Mutex};

#[cfg(not(target_os = "android"))]
static BROWSER: Mutex<()> = Mutex::const_new(());

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

#[cfg(target_os = "android")]
pub async fn get(_url: &Url, _authorization: Option<String>, _navigation_root: &Url) -> Result<(u16, Vec<u8>, Option<String>, Url), String> {
    Err("browser feed verification is unavailable on Android".into())
}

#[cfg(not(target_os = "android"))]
pub async fn get(url: &Url, authorization: Option<String>, navigation_root: &Url) -> Result<(u16, Vec<u8>, Option<String>, Url), String> {
    let profile = PROFILE.get().ok_or("browser verification is unavailable on this device")?;
    let executable = helper()?;
    let _browser = BROWSER.lock().await;
    let navigation_url = (authorization.is_none() && navigation_root.origin() == url.origin()).then(|| navigation_root.to_string());
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
