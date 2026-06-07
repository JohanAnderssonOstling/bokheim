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
    UserIdMismatch { local: String, server: String },
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::Network(e) => write!(f, "network error: {e}"),
            AuthError::Io(e) => write!(f, "io error: {e}"),
            AuthError::BadStatus(s, msg) => write!(f, "server returned {s}: {msg}"),
            AuthError::UserIdMismatch { local, server } => write!(
                f,
                "local user_id={local} does not match server user_id={server}; \
                 logging in here would mix two different accounts."
            ),
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
        fs::write(&path, bytes)
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
    #[serde(skip_serializing_if = "Option::is_none")]
    adopt_user_id: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct AuthResponse {
    token: String,
    user_id: String,
    username: String,
}

pub async fn login(server_url: &str, username: &str, password: &str, local_user_id: Option<&str>) -> Result<Session, AuthError> {
    let client = reqwest::Client::builder().build()?;
    let resp = client
        .post(format!("{}/api/auth/login", server_url.trim_end_matches('/')))
        .json(&LoginRequest { username, password })
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    let body: AuthResponse = resp.json().await?;
    if let Some(local) = local_user_id {
        if !local.is_empty() && local != body.user_id {
            return Err(AuthError::UserIdMismatch {
                local: local.to_string(),
                server: body.user_id,
            });
        }
    }
    let session = Session {
        server_url: server_url.trim_end_matches('/').to_string(),
        user_id: body.user_id,
        username: body.username,
        token: body.token,
    };
    Ok(session)
}

pub async fn signup(server_url: &str, invite: &str, username: &str, password: &str, adopt_user_id: Option<&str>) -> Result<Session, AuthError> {
    let client = reqwest::Client::builder().build()?;
    let req = SignupRequest { username, password, invite, adopt_user_id };
    let resp = client
        .post(format!("{}/api/auth/signup", server_url.trim_end_matches('/')))
        .json(&req)
        .send()
        .await?;
    let resp = ensure_ok(resp).await?;
    let body: AuthResponse = resp.json().await?;
    let session = Session {
        server_url: server_url.trim_end_matches('/').to_string(),
        user_id: body.user_id,
        username: body.username,
        token: body.token,
    };
    Ok(session)
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

/// Reads the locally-cached user_id (the UUID the Backend tracks for this device)
/// without requiring a fully-constructed `Library`. Falls back to `None` if the
/// file doesn't exist.
pub fn local_user_id() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let mut p = PathBuf::from(home);
    p.push(".biblos");
    p.push("user_id");
    let s = fs::read_to_string(p).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

pub fn login_blocking(server_url: &str, username: &str, password: &str) -> Result<Session, AuthError> {
    let local = local_user_id();
    let server_url = server_url.to_string();
    let username = username.to_string();
    let password = password.to_string();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(AuthError::Io)?;
    rt.block_on(async move { login(&server_url, &username, &password, local.as_deref()).await })
}

pub fn signup_blocking(server_url: &str, invite: &str, username: &str, password: &str) -> Result<Session, AuthError> {
    let local = local_user_id();
    let server_url = server_url.to_string();
    let invite = invite.to_string();
    let username = username.to_string();
    let password = password.to_string();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(AuthError::Io)?;
    rt.block_on(async move { signup(&server_url, &invite, &username, &password, local.as_deref()).await })
}

pub fn logout_blocking(session: &Session) {
    let s = session.clone();
    if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
        let _ = rt.block_on(async move { logout(&s).await });
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
