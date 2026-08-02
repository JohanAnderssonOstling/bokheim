use crate::ServerUrl;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use sync_common::api::auth::{AuthResponse, LoginRequest, SignupRequest};
use sync_common::api::libraries::{LibrarySummary, UpsertLibraryRequest};
use uuid::Uuid;

#[derive(Clone)]
pub struct Session {
    server_url: ServerUrl,
    user_id: String,
    username: String,
    token: String,
    expires_at: Option<i64>,
}

#[derive(Serialize, Deserialize)]
struct StoredSession {
    server_url: ServerUrl,
    user_id: String,
    username: String,
    token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at: Option<i64>,
}

#[derive(Debug)]
pub enum AuthError {
    Network(reqwest::Error),
    Io(std::io::Error),
    InvalidData(String),
    BadStatus(reqwest::StatusCode, String),
    Binding(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Network(error) => write!(formatter, "network error: {error}"),
            Self::Io(error) => write!(formatter, "io error: {error}"),
            Self::InvalidData(message) => write!(formatter, "invalid authentication data: {message}"),
            Self::BadStatus(status, message) => write!(formatter, "server returned {status}: {message}"),
            Self::Binding(message) => message.fmt(formatter),
        }
    }
}

impl std::error::Error for AuthError {}

impl AuthError {
    pub(crate) fn is_authentication_failure(&self) -> bool {
        matches!(self, Self::BadStatus(status, _) if *status == reqwest::StatusCode::UNAUTHORIZED || *status == reqwest::StatusCode::FORBIDDEN)
    }
}

impl From<reqwest::Error> for AuthError {
    fn from(error: reqwest::Error) -> Self {
        Self::Network(error)
    }
}

impl From<std::io::Error> for AuthError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Session").field("server_url", &self.server_url).field("user_id", &self.user_id).field("username", &self.username).field("token", &"[redacted]").field("expires_at", &self.expires_at).finish()
    }
}

impl Serialize for Session {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        StoredSession { server_url: self.server_url.clone(), user_id: self.user_id.clone(), username: self.username.clone(), token: self.token.clone(), expires_at: self.expires_at }.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Session {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = StoredSession::deserialize(deserializer)?;
        Self::try_new_with_expiry(stored.server_url, stored.user_id, stored.username, stored.token, stored.expires_at).map_err(serde::de::Error::custom)
    }
}

fn validate_identity(value: &str, name: &str) -> Result<(), AuthError> {
    if value.is_empty() || value.len() > 255 || value.chars().any(|character| character.is_control() || character == '/' || character == '\\') {
        return Err(AuthError::InvalidData(format!("{name} must be non-empty, bounded text without control characters or path separators")));
    }
    Ok(())
}

pub(crate) fn validate_user_id(value: &str) -> Result<(), AuthError> {
    validate_identity(value, "user id")
}

fn validate_username(value: &str) -> Result<(), AuthError> {
    if value.len() < 3 || value.len() > 64 || !value.chars().all(|character| character.is_alphanumeric() || matches!(character, '.' | '_' | '-')) {
        return Err(AuthError::InvalidData("username must be 3-64 characters using letters, numbers, '.', '_' or '-'".to_string()));
    }
    Ok(())
}

fn validate_token(value: &str) -> Result<(), AuthError> {
    if value.len() != 43 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')) {
        return Err(AuthError::InvalidData("bearer token has an invalid shape".to_string()));
    }
    Ok(())
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "session path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".session-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

impl Session {
    pub fn try_new(server_url: ServerUrl, user_id: String, username: String, token: String) -> Result<Self, AuthError> {
        Self::try_new_with_expiry(server_url, user_id, username, token, None)
    }

    pub(crate) fn try_new_with_expiry(server_url: ServerUrl, user_id: String, username: String, token: String, expires_at: Option<i64>) -> Result<Self, AuthError> {
        validate_identity(&user_id, "user id")?;
        validate_username(&username)?;
        validate_token(&token)?;
        if expires_at.is_some_and(|expires_at| expires_at <= 0) {
            return Err(AuthError::InvalidData("session expiry must be a positive Unix timestamp".to_string()));
        }
        Ok(Self { server_url, user_id, username, token, expires_at })
    }

    pub fn server_url(&self) -> &ServerUrl {
        &self.server_url
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    pub fn expires_at(&self) -> Option<i64> {
        self.expires_at
    }

    pub(crate) fn expires_within(&self, seconds: i64) -> bool {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_secs() as i64).unwrap_or(i64::MAX);
        self.expires_at.is_some_and(|expires_at| expires_at <= now.saturating_add(seconds.max(0)))
    }

    pub fn load_from(path: &Path) -> std::io::Result<Option<Self>> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        serde_json::from_slice(&bytes).map(Some).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }

    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        let bytes = serde_json::to_vec_pretty(self).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        write_private_atomic(path, &bytes)
    }

    pub fn clear_from(path: &Path) -> std::io::Result<()> {
        match fs::remove_file(path) {
            Ok(()) => {
                #[cfg(unix)]
                fs::File::open(path.parent().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "session path has no parent"))?)?.sync_all()?;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Legacy desktop session location retained for the Floem UI. New
    /// platform-aware shells pass their backend-owned path explicitly.
    pub fn load() -> std::io::Result<Option<Self>> {
        Self::load_from(&desktop_session_path()?)
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&desktop_session_path()?)
    }

    pub fn clear() -> std::io::Result<()> {
        Self::clear_from(&desktop_session_path()?)
    }
}

fn desktop_session_path() -> std::io::Result<PathBuf> {
    dirs::home_dir().map(|home| home.join(crate::APP_HIDDEN_SESSION_PATH)).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "desktop home directory is unavailable"))
}

async fn post_auth(url: reqwest::Url, body: &impl Serialize) -> Result<AuthResponse, AuthError> {
    let client = reqwest::Client::builder().build()?;
    let response = client.post(url).json(body).send().await?;
    let response = ensure_ok(response).await?;
    Ok(response.json().await?)
}

async fn login_at(server_url: &ServerUrl, username: &str, password: &str) -> Result<Session, AuthError> {
    let body = post_auth(server_url.endpoint("api/auth/login"), &LoginRequest { username: username.to_owned(), password: password.to_owned() }).await?;
    Session::try_new_with_expiry(server_url.clone(), body.user_id, body.username, body.token, Some(body.expires_at))
}

pub async fn login(username: &str, password: &str) -> Result<Session, AuthError> {
    login_at(&crate::api_server_url(), username, password).await
}

pub(crate) async fn signup_at(server_url: &ServerUrl, invite: &str, username: &str, password: &str) -> Result<Session, AuthError> {
    let body = post_auth(server_url.endpoint("api/auth/signup"), &SignupRequest { username: username.to_owned(), password: password.to_owned(), invite: invite.to_owned() }).await?;
    Session::try_new_with_expiry(server_url.clone(), body.user_id, body.username, body.token, Some(body.expires_at))
}

pub async fn signup(invite: &str, username: &str, password: &str) -> Result<Session, AuthError> {
    signup_at(&crate::api_server_url(), invite, username, password).await
}

fn require_api_session(session: &Session) -> Result<(), AuthError> {
    if session.server_url() == &crate::api_server_url() {
        Ok(())
    } else {
        Err(AuthError::Binding(format!("server communication must use {}", crate::API_SERVER_ORIGIN)))
    }
}

pub async fn logout(session: &Session) -> Result<(), AuthError> {
    require_api_session(session)?;
    let client = reqwest::Client::builder().build()?;
    let response = client.post(session.server_url.endpoint("api/auth/logout")).bearer_auth(&session.token).send().await?;
    ensure_ok(response).await?;
    Ok(())
}

pub async fn list_libraries(session: &Session) -> Result<Vec<LibrarySummary>, AuthError> {
    require_api_session(session)?;
    let client = reqwest::Client::builder().build()?;
    let response = client.get(session.server_url.endpoint("api/libraries")).bearer_auth(&session.token).send().await?;
    let response = ensure_ok(response).await?;
    Ok(response.json().await?)
}

pub async fn upsert_library(session: &Session, library_id: sync_common::LibraryId, library_name: String) -> Result<LibrarySummary, AuthError> {
    require_api_session(session)?;
    let client = reqwest::Client::builder().build()?;
    let request = UpsertLibraryRequest { library_id, library_name };
    let response = client.post(session.server_url.endpoint("api/libraries")).bearer_auth(&session.token).json(&request).send().await?;
    let response = ensure_ok(response).await?;
    Ok(response.json().await?)
}

fn blocking_runtime() -> Result<tokio::runtime::Runtime, AuthError> {
    tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(AuthError::Io)
}

pub fn login_blocking(username: &str, password: &str) -> Result<Session, AuthError> {
    blocking_runtime()?.block_on(login(username, password))
}

pub fn signup_blocking(invite: &str, username: &str, password: &str) -> Result<Session, AuthError> {
    blocking_runtime()?.block_on(signup(invite, username, password))
}

pub fn logout_blocking(session: &Session) -> Result<(), AuthError> {
    blocking_runtime()?.block_on(logout(session))
}

async fn ensure_ok(response: reqwest::Response) -> Result<reqwest::Response, AuthError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    Err(AuthError::BadStatus(status, body))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn session_deserialization_validates_every_field() {
        let valid = serde_json::json!({ "server_url": "https://example.com", "user_id": "user-1", "username": "reader", "token": TEST_TOKEN });
        let session: Session = serde_json::from_value(valid.clone()).unwrap();
        assert_eq!(session.server_url().as_str(), "https://example.com/");
        assert_eq!(session.user_id(), "user-1");
        for (field, invalid) in [("server_url", "file:///tmp/server"), ("user_id", "bad/id"), ("username", "bad name"), ("token", "short")] {
            let mut value = valid.clone();
            value[field] = serde_json::Value::String(invalid.to_string());
            assert!(serde_json::from_value::<Session>(value).is_err(), "invalid {field} was accepted");
        }
    }

    #[test]
    fn session_debug_output_redacts_the_token() {
        let session = Session::try_new(ServerUrl::parse("https://example.com").unwrap(), "user-1".to_string(), "reader".to_string(), TEST_TOKEN.to_string()).unwrap();
        let debug = format!("{session:?}");
        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains(TEST_TOKEN));
    }

    #[test]
    fn session_expiry_round_trips_and_is_enforced() {
        let expires_at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64 + 30;
        let session = Session::try_new_with_expiry(ServerUrl::parse("https://example.com").unwrap(), "user-1".to_string(), "reader".to_string(), TEST_TOKEN.to_string(), Some(expires_at)).unwrap();
        assert_eq!(session.expires_at(), Some(expires_at));
        assert!(session.expires_within(60));
        let restored: Session = serde_json::from_str(&serde_json::to_string(&session).unwrap()).unwrap();
        assert_eq!(restored.expires_at(), Some(expires_at));
        let invalid_expiry = serde_json::json!({ "server_url": "https://example.com", "user_id": "user-1", "username": "reader", "token": TEST_TOKEN, "expires_at": 0 });
        assert!(serde_json::from_value::<Session>(invalid_expiry).is_err());
    }

    #[test]
    fn production_requests_reject_non_api_sessions() {
        let session = Session::try_new(ServerUrl::parse("https://example.com").unwrap(), "user-1".to_string(), "reader".to_string(), TEST_TOKEN.to_string()).unwrap();
        assert!(require_api_session(&session).is_err());
    }
}
