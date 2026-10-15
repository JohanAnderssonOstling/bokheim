use crate::Url;
use reqwest::StatusCode;
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt::{self, Formatter};
use sync_common::transport::WireError;
use url::ParseError;
use web_time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub enum AuthError {
    Http(reqwest::Error),
    Wire(WireError),
    Url(ParseError),
    InvalidData(String),
}

impl fmt::Display for AuthError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Http(error) => error.fmt(formatter),
            Self::Wire(error) => error.fmt(formatter),
            Self::Url(error) => error.fmt(formatter),
            Self::InvalidData(message) => write!(formatter, "invalid authentication data: {message}"),
        }
    }
}

impl std::error::Error for AuthError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Http(error) => Some(error),
            Self::Wire(error) => Some(error),
            Self::Url(error) => Some(error),
            Self::InvalidData(_) => None,
        }
    }
}

impl AuthError {
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Http(error) if error.status() == Some(StatusCode::NOT_FOUND))
    }

    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::Http(error) if error.status() == Some(StatusCode::UNAUTHORIZED))
    }

    /// The server refused the request as made (400 or 409): sending it again
    /// cannot succeed.
    pub fn is_rejected(&self) -> bool {
        matches!(self, Self::Http(error) if matches!(error.status(), Some(StatusCode::BAD_REQUEST | StatusCode::CONFLICT)))
    }
}

impl From<reqwest::Error> for AuthError {
    fn from(error: reqwest::Error) -> Self {
        Self::Http(error)
    }
}

impl From<WireError> for AuthError {
    fn from(error: WireError) -> Self {
        Self::Wire(error)
    }
}

impl From<ParseError> for AuthError {
    fn from(error: ParseError) -> Self {
        Self::Url(error)
    }
}

#[derive(Clone, Serialize)]
#[serde(rename = "StoredSession")]
pub struct Session {
    server_url: Url,
    user_id: String,
    email: String,
    token: String,
    expires_at: Option<i64>,
    refresh_token: Option<String>,
    refresh_expires_at: Option<i64>,
}

#[derive(Deserialize)]
struct StoredSession {
    server_url: Url,
    user_id: String,
    email: String,
    token: String,
    #[serde(deserialize_with = "Option::deserialize")]
    expires_at: Option<i64>,
    #[serde(deserialize_with = "Option::deserialize")]
    refresh_token: Option<String>,
    #[serde(deserialize_with = "Option::deserialize")]
    refresh_expires_at: Option<i64>,
}

impl fmt::Debug for Session {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Session")
            .field("server_url", &self.server_url)
            .field("user_id", &self.user_id)
            .field("email", &self.email)
            .field("token", &"[redacted]")
            .field("expires_at", &self.expires_at)
            .field("refresh_token", &self.refresh_token.as_ref().map(|_| "[redacted]"))
            .field("refresh_expires_at", &self.refresh_expires_at)
            .finish()
    }
}

impl<'de> Deserialize<'de> for Session {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = StoredSession::deserialize(deserializer)?;
        Self::try_new_with_tokens(stored.server_url, stored.user_id, stored.email, stored.token, stored.expires_at, stored.refresh_token, stored.refresh_expires_at).map_err(serde::de::Error::custom)
    }
}

pub(crate) fn validate_identity(value: &str, name: &str) -> Result<(), AuthError> {
    if value.is_empty() || value.len() > 255 || value.chars().any(|character| character.is_control() || character == '/' || character == '\\') {
        return Err(AuthError::InvalidData(format!("{name} must be non-empty, bounded text without control characters or path separators")));
    }
    Ok(())
}

pub fn validate_user_id(value: &str) -> Result<(), AuthError> {
    validate_identity(value, "user id")
}

pub(crate) fn validate_email(value: &str) -> Result<(), AuthError> {
    if value.is_empty() || value.len() > account_contract::EMAIL_MAX_BYTES {
        return Err(AuthError::InvalidData("email must be a non-empty, bounded address".to_string()));
    }
    Ok(())
}

fn validate_token(value: &str) -> Result<(), AuthError> {
    if value.len() != 43 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')) {
        return Err(AuthError::InvalidData("bearer token has an invalid shape".to_string()));
    }
    Ok(())
}

fn expiry_within(expires_at: Option<i64>, seconds: i64) -> bool {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_secs() as i64).unwrap_or(i64::MAX);
    expires_at.is_some_and(|expires_at| expires_at <= now.saturating_add(seconds.max(0)))
}

impl Session {
    pub fn try_new(server_url: Url, user_id: String, email: String, token: String) -> Result<Self, AuthError> {
        Self::try_new_with_tokens(server_url, user_id, email, token, None, None, None)
    }

    pub fn try_new_with_tokens(server_url: Url, user_id: String, email: String, token: String, expires_at: Option<i64>, refresh_token: Option<String>, refresh_expires_at: Option<i64>) -> Result<Self, AuthError> {
        validate_identity(&user_id, "user id")?;
        validate_email(&email)?;
        validate_token(&token)?;
        if expires_at.is_some_and(|expires_at| expires_at <= 0) {
            return Err(AuthError::InvalidData("session expiry must be a positive Unix timestamp".to_string()));
        }
        match (&refresh_token, refresh_expires_at) {
            (Some(refresh_token), Some(refresh_expires_at)) => {
                validate_token(refresh_token)?;
                if refresh_expires_at <= 0 {
                    return Err(AuthError::InvalidData("refresh expiry must be a positive Unix timestamp".to_string()));
                }
            }
            (None, None) => {}
            _ => return Err(AuthError::InvalidData("refresh token and expiry must be stored together".to_string())),
        }
        Ok(Self { server_url, user_id, email, token, expires_at, refresh_token, refresh_expires_at })
    }

    pub fn server_url(&self) -> &Url {
        &self.server_url
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    pub fn email(&self) -> &str {
        &self.email
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn expires_at(&self) -> Option<i64> {
        self.expires_at
    }

    pub fn refresh_token(&self) -> Option<&str> {
        self.refresh_token.as_deref()
    }

    pub fn refresh_expires_at(&self) -> Option<i64> {
        self.refresh_expires_at
    }

    pub fn expires_within(&self, seconds: i64) -> bool {
        expiry_within(self.expires_at, seconds)
    }

    pub fn refresh_expires_within(&self, seconds: i64) -> bool {
        expiry_within(self.refresh_expires_at, seconds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn debug_redacts_token_and_expiry_is_validated() {
        let session = Session::try_new(Url::parse("https://example.com").unwrap(), "user-1".into(), "reader@example.com".into(), TEST_TOKEN.into()).unwrap();
        assert!(!format!("{session:?}").contains(TEST_TOKEN));
        let restored: Session = serde_json::from_value(serde_json::to_value(&session).unwrap()).unwrap();
        assert_eq!(restored.server_url(), session.server_url());
        assert_eq!(restored.token(), session.token());
        let invalid = serde_json::json!({ "server_url": "https://example.com", "user_id": "user-1", "email": "reader@example.com", "token": TEST_TOKEN, "expires_at": 0, "refresh_token": null, "refresh_expires_at": null });
        assert!(serde_json::from_value::<Session>(invalid).is_err());
    }
}
