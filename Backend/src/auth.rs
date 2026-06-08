use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub server_url: String,
    pub user_id: String,
    pub username: String,
    pub token: String,
}

#[derive(Debug)]
pub enum AuthError {
    Network(reqwest::Error),
    Io(std::io::Error),
    BadStatus(reqwest::StatusCode, String),
    /// The authenticated account doesn't match the account this local library is
    /// bound to (would mix two accounts' data).
    Binding(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::Network(e) => write!(f, "network error: {e}"),
            AuthError::Io(e) => write!(f, "io error: {e}"),
            AuthError::BadStatus(s, msg) => write!(f, "server returned {s}: {msg}"),
            AuthError::Binding(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for AuthError {}

impl From<reqwest::Error> for AuthError {
    fn from(e: reqwest::Error) -> Self {
        AuthError::Network(e)
    }
}

impl From<std::io::Error> for AuthError {
    fn from(e: std::io::Error) -> Self {
        AuthError::Io(e)
    }
}

fn session_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let mut p = PathBuf::from(home);
    p.push(".biblos");
    let _ = fs::create_dir_all(&p);
    p.push("session.json");
    Some(p)
}

impl Session {
    pub fn load() -> Option<Self> {
        let path = session_path()?;
        let bytes = fs::read(&path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = session_path().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no HOME"))?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(&path, bytes)?;
        // The file holds a live bearer token; keep it owner-only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    pub fn clear() {
        if let Some(path) = session_path() {
            let _ = fs::remove_file(&path);
        }
    }
}

#[derive(Debug, Serialize)]
struct LoginRequest<'a> {
    username: &'a str,
    password: &'a str,
}

#[derive(Debug, Serialize)]
struct SignupRequest<'a> {
    username: &'a str,
    password: &'a str,
    invite: &'a str,
}

#[derive(Debug, Deserialize)]
struct AuthResponse {
    token: String,
    user_id: String,
    username: String,
}

impl AuthResponse {
    fn into_session(self, server_url: String) -> Session {
        Session { server_url, user_id: self.user_id, username: self.username, token: self.token }
    }
}

async fn post_auth(url: String, body: &impl Serialize) -> Result<AuthResponse, AuthError> {
    let client = reqwest::Client::builder().build()?;
    let resp = client.post(url).json(body).send().await?;
    let resp = ensure_ok(resp).await?;
    Ok(resp.json().await?)
}

pub async fn login(server_url: &str, username: &str, password: &str) -> Result<Session, AuthError> {
    let base = server_url.trim_end_matches('/');
    let body = post_auth(format!("{base}/api/auth/login"), &LoginRequest { username, password }).await?;
    Ok(body.into_session(base.to_string()))
}

pub async fn signup(server_url: &str, invite: &str, username: &str, password: &str) -> Result<Session, AuthError> {
    let base = server_url.trim_end_matches('/');
    let body = post_auth(format!("{base}/api/auth/signup"), &SignupRequest { username, password, invite }).await?;
    Ok(body.into_session(base.to_string()))
}

pub async fn logout(session: &Session) -> Result<(), AuthError> {
    let client = reqwest::Client::builder().build()?;
    let _ = client
        .post(format!("{}/api/auth/logout", session.server_url))
        .bearer_auth(&session.token)
        .send()
        .await;
    Ok(())
}

fn blocking_runtime() -> Result<tokio::runtime::Runtime, AuthError> {
    tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(AuthError::Io)
}

pub fn login_blocking(server_url: &str, username: &str, password: &str) -> Result<Session, AuthError> {
    blocking_runtime()?.block_on(login(server_url, username, password))
}

pub fn signup_blocking(server_url: &str, invite: &str, username: &str, password: &str) -> Result<Session, AuthError> {
    blocking_runtime()?.block_on(signup(server_url, invite, username, password))
}

pub fn logout_blocking(session: &Session) {
    if let Ok(rt) = blocking_runtime() {
        let _ = rt.block_on(logout(session));
    }
}

async fn ensure_ok(resp: reqwest::Response) -> Result<reqwest::Response, AuthError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let body = resp.text().await.unwrap_or_default();
    Err(AuthError::BadStatus(status, body))
}
