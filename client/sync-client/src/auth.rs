use crate::ServerUrl;
use account_contract::{AuthResponse, EmailRequest, EmailVerificationRequest, LoginRequest, MeResponse, PublicRegistrationRequest, RefreshRequest, ResetPasswordRequest, StorageUsageResponse};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sync_common::api::assets::{validate_blob_manifest_response, BlobManifestEntry, BlobManifestRequest, BlobManifestResponse};
use sync_common::api::libraries::{DeleteLibraryResponse, LibrarySummary, UpsertLibraryRequest};
use sync_common::transport as wire;
use sync_common::{LibraryId, ReplicaId, StateInventoryRequest, StateInventoryResponse, SyncCursor, SyncExchangeRequest, SyncExchangeResponse};

#[derive(Clone)]
pub struct Session {
    server_url: ServerUrl,
    user_id: String,
    email: String,
    token: String,
    expires_at: Option<i64>,
    refresh_token: Option<String>,
    refresh_expires_at: Option<i64>,
}

#[derive(Serialize, Deserialize)]
struct StoredSession {
    server_url: ServerUrl,
    user_id: String,
    email: String,
    token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expires_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    refresh_expires_at: Option<i64>,
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

/// Control-flow relevant failures from synchronization endpoints. Account
/// APIs retain `AuthError`; sync callers should never need to inspect an HTTP
/// status or downcast an erased error to decide whether to refresh or recover.
#[derive(Debug)]
pub enum SyncRequestError {
    AuthenticationRequired,
    CursorRejected,
    EndpointNotFound,
    Retryable(AuthError),
    InvalidResponse(AuthError),
}

impl std::fmt::Display for SyncRequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthenticationRequired => formatter.write_str("synchronization authentication is required"),
            Self::CursorRejected => formatter.write_str("the synchronization cursor was rejected"),
            Self::EndpointNotFound => formatter.write_str("the synchronization endpoint was not found"),
            Self::Retryable(error) | Self::InvalidResponse(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SyncRequestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Retryable(error) | Self::InvalidResponse(error) => Some(error),
            Self::AuthenticationRequired | Self::CursorRejected | Self::EndpointNotFound => None,
        }
    }
}

impl From<AuthError> for SyncRequestError {
    fn from(error: AuthError) -> Self {
        match error {
            AuthError::BadStatus(reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN, _) => Self::AuthenticationRequired,
            AuthError::BadStatus(reqwest::StatusCode::GONE, _) => Self::CursorRejected,
            AuthError::BadStatus(reqwest::StatusCode::NOT_FOUND, _) => Self::EndpointNotFound,
            AuthError::Network(error) => Self::Retryable(AuthError::Network(error)),
            AuthError::BadStatus(status, message) if status.is_server_error() || status == reqwest::StatusCode::REQUEST_TIMEOUT || status == reqwest::StatusCode::TOO_MANY_REQUESTS => Self::Retryable(AuthError::BadStatus(status, message)),
            error => Self::InvalidResponse(error),
        }
    }
}

impl AuthError {
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::BadStatus(status, _) if *status == reqwest::StatusCode::NOT_FOUND)
    }

    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::BadStatus(status, _) if *status == reqwest::StatusCode::UNAUTHORIZED)
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

impl Serialize for Session {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        StoredSession {
            server_url: self.server_url.clone(),
            user_id: self.user_id.clone(),
            email: self.email.clone(),
            token: self.token.clone(),
            expires_at: self.expires_at,
            refresh_token: self.refresh_token.clone(),
            refresh_expires_at: self.refresh_expires_at,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Session {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let stored = StoredSession::deserialize(deserializer)?;
        Self::try_new_with_tokens(stored.server_url, stored.user_id, stored.email, stored.token, stored.expires_at, stored.refresh_token, stored.refresh_expires_at).map_err(serde::de::Error::custom)
    }
}

fn validate_identity(value: &str, name: &str) -> Result<(), AuthError> {
    if value.is_empty() || value.len() > 255 || value.chars().any(|character| character.is_control() || character == '/' || character == '\\') {
        return Err(AuthError::InvalidData(format!("{name} must be non-empty, bounded text without control characters or path separators")));
    }
    Ok(())
}

pub fn validate_user_id(value: &str) -> Result<(), AuthError> {
    validate_identity(value, "user id")
}

fn validate_email(value: &str) -> Result<(), AuthError> {
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

impl Session {
    pub fn try_new(server_url: ServerUrl, user_id: String, email: String, token: String) -> Result<Self, AuthError> {
        Self::try_new_with_expiry(server_url, user_id, email, token, None)
    }

    pub fn try_new_with_expiry(server_url: ServerUrl, user_id: String, email: String, token: String, expires_at: Option<i64>) -> Result<Self, AuthError> {
        Self::try_new_with_tokens(server_url, user_id, email, token, expires_at, None, None)
    }

    pub fn try_new_with_tokens(server_url: ServerUrl, user_id: String, email: String, token: String, expires_at: Option<i64>, refresh_token: Option<String>, refresh_expires_at: Option<i64>) -> Result<Self, AuthError> {
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

    pub fn server_url(&self) -> &ServerUrl {
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
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_secs() as i64).unwrap_or(i64::MAX);
        self.expires_at.is_some_and(|expires_at| expires_at <= now.saturating_add(seconds.max(0)))
    }

    pub fn refresh_expires_within(&self, seconds: i64) -> bool {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_secs() as i64).unwrap_or(i64::MAX);
        self.refresh_expires_at.is_some_and(|expires_at| expires_at <= now.saturating_add(seconds.max(0)))
    }

    pub fn with_server_url(&self, server_url: ServerUrl) -> Result<Self, AuthError> {
        Self::try_new_with_tokens(server_url, self.user_id.clone(), self.email.clone(), self.token.clone(), self.expires_at, self.refresh_token.clone(), self.refresh_expires_at)
    }
}

fn session_from_response(server_url: ServerUrl, body: AuthResponse) -> Result<Session, AuthError> {
    Session::try_new_with_tokens(server_url, body.user_id, body.email, body.token, Some(body.expires_at), Some(body.refresh_token), Some(body.refresh_expires_at))
}

fn api_headers(request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    request.header(reqwest::header::ACCEPT, wire::MEDIA_TYPE)
}

fn api_body<T: Serialize + ?Sized>(request: reqwest::RequestBuilder, value: &T) -> Result<reqwest::RequestBuilder, AuthError> {
    let encoded = wire::encode(value).map_err(|error| AuthError::InvalidData(error.to_string()))?;
    Ok(api_headers(request).header(reqwest::header::CONTENT_TYPE, wire::MEDIA_TYPE).body(encoded))
}

async fn send_api<T: Serialize + ?Sized, R: DeserializeOwned>(request: reqwest::RequestBuilder, value: &T) -> Result<R, AuthError> {
    decode_api_response(api_body(request, value)?.send().await?).await
}

async fn send_api_status<T: Serialize + ?Sized>(request: reqwest::RequestBuilder, value: &T) -> Result<(), AuthError> {
    ensure_ok(api_body(request, value)?.send().await?).await?;
    Ok(())
}

async fn receive_api<R: DeserializeOwned>(request: reqwest::RequestBuilder) -> Result<R, AuthError> {
    decode_api_response(api_headers(request).send().await?).await
}

async fn post_auth(url: reqwest::Url, body: &impl Serialize) -> Result<AuthResponse, AuthError> {
    send_api(reqwest::Client::builder().build()?.post(url), body).await
}

pub async fn login(server_url: &ServerUrl, email: &str, password: &str) -> Result<Session, AuthError> {
    let body = post_auth(server_url.endpoint("api/auth/login"), &LoginRequest { email: email.to_owned(), password: password.to_owned() }).await?;
    session_from_response(server_url.clone(), body)
}

pub async fn request_public_registration(server_url: &ServerUrl, email: &str, password: &str) -> Result<(), AuthError> {
    send_api_status(reqwest::Client::builder().build()?.post(server_url.endpoint("api/auth/register")), &PublicRegistrationRequest { email: email.to_owned(), password: password.to_owned() }).await
}

pub async fn resend_verification(server_url: &ServerUrl, email: &str) -> Result<(), AuthError> {
    send_api_status(reqwest::Client::builder().build()?.post(server_url.endpoint("api/auth/resend-verification")), &EmailRequest { email: email.to_owned() }).await
}

pub async fn verify_email(server_url: &ServerUrl, email: &str, pin: &str) -> Result<Session, AuthError> {
    let body = post_auth(server_url.endpoint("api/auth/verify-email"), &EmailVerificationRequest { email: email.to_owned(), pin: pin.to_owned() }).await?;
    session_from_response(server_url.clone(), body)
}

pub async fn request_password_reset(server_url: &ServerUrl, email: &str) -> Result<(), AuthError> {
    send_api_status(reqwest::Client::builder().build()?.post(server_url.endpoint("api/auth/request-password-reset")), &EmailRequest { email: email.to_owned() }).await
}

pub async fn reset_password(server_url: &ServerUrl, token: &str, new_password: &str) -> Result<(), AuthError> {
    send_api_status(reqwest::Client::builder().build()?.post(server_url.endpoint("api/auth/reset-password")), &ResetPasswordRequest { token: token.to_owned(), new_password: new_password.to_owned() }).await
}

pub async fn logout(session: &Session) -> Result<(), AuthError> {
    let request = reqwest::Client::builder().build()?.post(session.server_url.endpoint("api/auth/logout"));
    let response = match session.refresh_token.as_deref() {
        Some(refresh_token) => api_body(request, &RefreshRequest { refresh_token: refresh_token.to_owned() })?.send().await?,
        None => api_headers(request.bearer_auth(&session.token)).send().await?,
    };
    ensure_ok(response).await?;
    Ok(())
}

pub async fn refresh_session(session: &Session) -> Result<Session, AuthError> {
    let refresh_token = session.refresh_token.as_deref().ok_or_else(|| AuthError::InvalidData("this legacy session has no refresh token; sign in again".to_owned()))?;
    if session.refresh_expires_within(0) {
        return Err(AuthError::InvalidData("refresh token has expired; sign in again".to_owned()));
    }
    let body: AuthResponse = send_api(reqwest::Client::builder().build()?.post(session.server_url.endpoint("api/auth/refresh")), &RefreshRequest { refresh_token: refresh_token.to_owned() }).await?;
    let refreshed = session_from_response(session.server_url.clone(), body)?;
    if refreshed.user_id != session.user_id {
        return Err(AuthError::InvalidData("refreshed session changed authenticated user".to_owned()));
    }
    Ok(refreshed)
}

pub async fn current_account(session: &Session) -> Result<MeResponse, AuthError> {
    let account: MeResponse = receive_api(reqwest::Client::builder().build()?.get(session.server_url.endpoint("api/auth/me")).bearer_auth(&session.token)).await?;
    validate_identity(&account.user_id, "user id")?;
    validate_email(&account.email)?;
    if account.user_id != session.user_id || account.email != session.email {
        return Err(AuthError::InvalidData("session identity does not match the identity authority".to_owned()));
    }
    Ok(account)
}

pub async fn account_storage_usage(session: &Session) -> Result<StorageUsageResponse, AuthError> {
    receive_api(reqwest::Client::builder().build()?.get(session.server_url.endpoint("api/account/storage")).bearer_auth(&session.token)).await
}

/// The per-library split of the charged bytes.
///
/// A route older servers do not have; the caller decides what an absent
/// breakdown means rather than this layer inventing zeroes.
pub async fn account_library_storage_usage(session: &Session) -> Result<account_contract::LibraryStorageUsageResponse, AuthError> {
    receive_api(reqwest::Client::builder().build()?.get(session.server_url.endpoint("api/account/storage/libraries")).bearer_auth(&session.token)).await
}

pub async fn list_account_sessions(session: &Session) -> Result<Vec<account_contract::SessionSummary>, AuthError> {
    receive_api(reqwest::Client::builder().build()?.get(session.server_url.endpoint("api/auth/sessions")).bearer_auth(&session.token)).await
}

pub async fn revoke_account_session(session: &Session, session_id: &str) -> Result<account_contract::SessionRevocationResponse, AuthError> {
    validate_identity(session_id, "session id")?;
    let endpoint = format!("api/auth/sessions/{}", urlencoding::encode(session_id));
    receive_api(reqwest::Client::builder().build()?.delete(session.server_url.endpoint(&endpoint)).bearer_auth(&session.token)).await
}

pub async fn revoke_other_account_sessions(session: &Session) -> Result<account_contract::OtherSessionsRevocationResponse, AuthError> {
    receive_api(reqwest::Client::builder().build()?.delete(session.server_url.endpoint("api/auth/sessions")).bearer_auth(&session.token)).await
}

pub async fn list_libraries(session: &Session) -> Result<Vec<LibrarySummary>, AuthError> {
    receive_api(reqwest::Client::builder().build()?.get(session.server_url.endpoint("api/libraries")).bearer_auth(&session.token)).await
}

pub async fn list_deleted_libraries(session: &Session) -> Result<sync_common::api::libraries::DeletedLibrariesResponse, AuthError> {
    receive_api(reqwest::Client::builder().build()?.get(session.server_url.endpoint("api/libraries/deleted")).bearer_auth(&session.token)).await
}

pub async fn upsert_library(session: &Session, library_id: sync_common::LibraryId, library_name: String) -> Result<LibrarySummary, AuthError> {
    let request = UpsertLibraryRequest { library_id, library_name };
    send_api(reqwest::Client::builder().build()?.post(session.server_url.endpoint("api/libraries")).bearer_auth(&session.token), &request).await
}

pub async fn delete_library(session: &Session, library_id: &sync_common::LibraryId) -> Result<DeleteLibraryResponse, AuthError> {
    receive_api(reqwest::Client::builder().build()?.delete(session.server_url.endpoint(&format!("api/libraries/{library_id}"))).bearer_auth(&session.token)).await
}

pub async fn negotiate_blobs(client: &reqwest::Client, session: &Session, library_id: &LibraryId, blobs: Vec<BlobManifestEntry>) -> Result<BlobManifestResponse, SyncRequestError> {
    let request = BlobManifestRequest { blobs };
    let response: BlobManifestResponse = send_api(client.post(session.server_url.endpoint(&format!("api/libraries/{library_id}/blobs/negotiate"))).bearer_auth(&session.token), &request).await?;
    validate_blob_manifest_response(&request, &response).map_err(|error| AuthError::InvalidData(error.to_string()))?;
    Ok(response)
}

pub async fn exchange_changes(client: &reqwest::Client, session: &Session, library_id: LibraryId, replica_id: ReplicaId, mutations: Vec<sync_common::WireMutation>, cursor: SyncCursor) -> Result<SyncExchangeResponse, SyncRequestError> {
    let request = SyncExchangeRequest { library_id, replica_id, mutations, cursor };
    Ok(post_api(client, session, "api/sync/exchange", &request).await?)
}

pub async fn compare_state_inventory(client: &reqwest::Client, session: &Session, library_id: LibraryId, cells: Vec<sync_common::StateCell>) -> Result<StateInventoryResponse, SyncRequestError> {
    let request = StateInventoryRequest { library_id, cells };
    Ok(post_api(client, session, "api/sync/inventory", &request).await?)
}

async fn post_api<T: Serialize, R: DeserializeOwned>(client: &reqwest::Client, session: &Session, path: &str, value: &T) -> Result<R, AuthError> {
    send_api(client.post(session.server_url.endpoint(path)).bearer_auth(&session.token), value).await
}

async fn decode_api_response<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, AuthError> {
    let response = ensure_ok(response).await?;
    let content_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or_default().to_owned();
    if response.headers().get(reqwest::header::CONTENT_ENCODING).and_then(|value| value.to_str().ok()).map(str::trim).is_some_and(|value| !value.is_empty() && !value.eq_ignore_ascii_case("identity")) {
        return Err(AuthError::InvalidData("compressed API responses are not supported".to_owned()));
    }
    let body = response.bytes().await?;
    if !wire::is_current_media_type(&content_type) {
        return Err(AuthError::InvalidData(format!("unsupported API response content type: {content_type}")));
    }
    wire::decode(&body, wire::MAX_DECODED_RESPONSE_BYTES).map_err(|error| AuthError::InvalidData(error.to_string()))
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
    use axum::body::Bytes;
    use axum::http::{header, HeaderMap, HeaderValue};
    use axum::response::IntoResponse;
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use sync_common::{PullStateResponse, PushMutationsResponse};
    use test_support::fixture_content_hash;
    const TEST_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn sync_request_failures_are_closed_control_flow_categories() {
        assert!(matches!(SyncRequestError::from(AuthError::BadStatus(reqwest::StatusCode::UNAUTHORIZED, String::new())), SyncRequestError::AuthenticationRequired));
        assert!(matches!(SyncRequestError::from(AuthError::BadStatus(reqwest::StatusCode::GONE, String::new())), SyncRequestError::CursorRejected));
        assert!(matches!(SyncRequestError::from(AuthError::BadStatus(reqwest::StatusCode::NOT_FOUND, String::new())), SyncRequestError::EndpointNotFound));
        assert!(matches!(SyncRequestError::from(AuthError::BadStatus(reqwest::StatusCode::TOO_MANY_REQUESTS, String::new())), SyncRequestError::Retryable(_)));
        assert!(matches!(SyncRequestError::from(AuthError::BadStatus(reqwest::StatusCode::UNPROCESSABLE_ENTITY, String::new())), SyncRequestError::InvalidResponse(_)));
    }

    #[tokio::test]
    async fn account_storage_usage_uses_the_authenticated_binary_endpoint() {
        let app = Router::new().route(
            "/api/account/storage",
            get(|headers: HeaderMap| async move {
                let expected = format!("Bearer {TEST_TOKEN}");
                assert_eq!(headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()), Some(expected.as_str()));
                let encoded = wire::encode(&StorageUsageResponse { used_bytes: 40, reserved_bytes: 2, quota_bytes: 100 }).unwrap();
                let mut response = encoded.into_response();
                response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(wire::MEDIA_TYPE));
                response
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let session = Session::try_new(ServerUrl::parse(&format!("http://{address}")).unwrap(), "user-1".to_owned(), "reader@example.com".to_owned(), TEST_TOKEN.to_owned()).unwrap();

        assert_eq!(account_storage_usage(&session).await.unwrap(), StorageUsageResponse { used_bytes: 40, reserved_bytes: 2, quota_bytes: 100 });
        server.abort();
    }

    #[test]
    fn session_deserialization_validates_every_field() {
        let valid = serde_json::json!({ "server_url": "https://example.com", "user_id": "user-1", "email": "reader@example.com", "token": TEST_TOKEN });
        let session: Session = serde_json::from_value(valid.clone()).unwrap();
        assert_eq!(session.server_url().as_str(), "https://example.com/");
        for (field, invalid) in [("server_url", "file:///tmp/server"), ("user_id", "bad/id"), ("email", &"a".repeat(account_contract::EMAIL_MAX_BYTES + 1)), ("token", "short")] {
            let mut value = valid.clone();
            value[field] = serde_json::Value::String(invalid.to_string());
            assert!(serde_json::from_value::<Session>(value).is_err());
        }
    }

    #[test]
    fn debug_redacts_token_and_expiry_is_validated() {
        let session = Session::try_new(ServerUrl::parse("https://example.com").unwrap(), "user-1".into(), "reader@example.com".into(), TEST_TOKEN.into()).unwrap();
        assert!(!format!("{session:?}").contains(TEST_TOKEN));
        let invalid = serde_json::json!({ "server_url": "https://example.com", "user_id": "user-1", "email": "reader@example.com", "token": TEST_TOKEN, "expires_at": 0 });
        assert!(serde_json::from_value::<Session>(invalid).is_err());
    }

    #[test]
    fn refresh_credentials_round_trip_and_remain_redacted() {
        let session = Session::try_new_with_tokens(ServerUrl::parse("https://example.com").unwrap(), "user-1".into(), "reader@example.com".into(), TEST_TOKEN.into(), Some(100), Some("b".repeat(43)), Some(200)).unwrap();
        let stored = serde_json::to_string(&session).unwrap();
        let restored: Session = serde_json::from_str(&stored).unwrap();
        assert_eq!(restored.refresh_token(), Some("b".repeat(43).as_str()));
        let debug = format!("{session:?}");
        assert!(!debug.contains(TEST_TOKEN));
        assert!(!debug.contains(&"b".repeat(43)));
        assert!(Session::try_new_with_tokens(ServerUrl::parse("https://example.com").unwrap(), "user-1".into(), "reader@example.com".into(), TEST_TOKEN.into(), Some(100), Some("b".repeat(43)), None).is_err());
    }

    #[tokio::test]
    async fn refresh_rotates_both_credentials_without_changing_identity() {
        let received_old_refresh = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&received_old_refresh);
        let app = Router::new().route(
            "/api/auth/refresh",
            post(move |headers: HeaderMap, body: Bytes| {
                let observed = Arc::clone(&observed);
                async move {
                    assert!(headers.get(header::CONTENT_ENCODING).is_none());
                    let request: RefreshRequest = wire::decode(&body, wire::MAX_DECODED_REQUEST_BYTES).unwrap();
                    observed.store(request.refresh_token == "b".repeat(43), Ordering::SeqCst);
                    let encoded = wire::encode(&AuthResponse {
                        token: "c".repeat(43),
                        expires_at: 1_900_000_000,
                        refresh_token: "d".repeat(43),
                        refresh_expires_at: 1_910_000_000,
                        user_id: "user-1".to_owned(),
                        email: "reader@example.com".to_owned(),
                    })
                    .unwrap();
                    let mut response = encoded.into_response();
                    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(wire::MEDIA_TYPE));
                    response
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let session =
            Session::try_new_with_tokens(ServerUrl::parse(&format!("http://{address}")).unwrap(), "user-1".to_owned(), "reader@example.com".to_owned(), TEST_TOKEN.to_owned(), Some(1_800_000_000), Some("b".repeat(43)), Some(1_910_000_000))
                .unwrap();

        let refreshed = refresh_session(&session).await.unwrap();

        assert!(received_old_refresh.load(Ordering::SeqCst));
        assert_eq!(refreshed.token(), "c".repeat(43));
        assert_eq!(refreshed.refresh_token(), Some("d".repeat(43).as_str()));
        assert_eq!(refreshed.user_id(), session.user_id());
        assert_eq!(refreshed.email(), session.email());
        server.abort();
    }

    fn sync_mutations(count: usize) -> Vec<sync_common::WireMutation> {
        (1..=count)
            .map(|sequence| sync_common::WireMutation {
                mutation_id: sync_common::MutationId::new(),
                kind: sync_common::mutation_kind::BOOK_LIFECYCLE.to_owned(),
                entity_key: fixture_content_hash(sequence as u64).to_string(),
                entity_subkey: String::new(),
                value: vec![sequence as u8; 1024],
                conflict_rank: 0,
                blob_reference: Some(sync_common::DeclaredBlobReference { present: true, content_hash: Some(fixture_content_hash(sequence as u64)) }),
                changed_at: 1,
                replica_seq: sync_common::ReplicaSeq::new(sequence as u64).unwrap(),
            })
            .collect()
    }

    fn sync_session(address: std::net::SocketAddr) -> Session {
        Session::try_new(ServerUrl::parse(&format!("http://{address}")).unwrap(), "user-1".to_string(), "reader@example.com".to_string(), TEST_TOKEN.to_string()).unwrap()
    }

    #[tokio::test]
    async fn sync_client_sends_and_receives_versioned_binary() {
        let observed_binary = Arc::new(AtomicBool::new(false));
        let binary = observed_binary.clone();
        let app = Router::new().route(
            "/api/sync/exchange",
            post(move |headers: HeaderMap, body: Bytes| {
                let binary = binary.clone();
                async move {
                    assert!(headers.get(header::CONTENT_ENCODING).is_none());
                    binary.store(headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).is_some_and(|value| value.starts_with(sync_common::transport::MEDIA_TYPE_BASE)), Ordering::SeqCst);
                    let request: SyncExchangeRequest = sync_common::transport::decode(&body, sync_common::transport::MAX_DECODED_REQUEST_BYTES).unwrap();
                    let response = SyncExchangeResponse {
                        push: PushMutationsResponse { accepted: request.mutations.iter().map(|mutation| mutation.mutation_id).collect(), rejected: Vec::new() },
                        pull: PullStateResponse { mutations: Vec::new(), next_cursor: SyncCursor::default(), has_more: false },
                    };
                    let encoded = sync_common::transport::encode(&response).unwrap();
                    let mut response = encoded.into_response();
                    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(sync_common::transport::MEDIA_TYPE));
                    response
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mutations = sync_mutations(100);

        let response = exchange_changes(&reqwest::Client::new(), &sync_session(address), test_support::fixture_library_id("library"), test_support::fixture_replica_id("replica"), mutations, SyncCursor::default()).await.unwrap();

        assert!(observed_binary.load(Ordering::SeqCst));
        assert_eq!(response.push.accepted.len(), 100);
        server.abort();
    }

    #[tokio::test]
    async fn sync_client_does_not_retry_with_json_after_media_rejection() {
        use axum::body::Bytes;
        use axum::extract::State;
        use axum::http::StatusCode;
        use axum::response::IntoResponse;
        use std::sync::atomic::AtomicUsize;

        let attempts = Arc::new(AtomicUsize::new(0));
        let app = Router::new()
            .route(
                "/api/sync/exchange",
                post(|State(attempts): State<Arc<AtomicUsize>>, _body: Bytes| async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response()
                }),
            )
            .with_state(attempts.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let response = exchange_changes(&reqwest::Client::new(), &sync_session(address), test_support::fixture_library_id("library"), test_support::fixture_replica_id("replica"), sync_mutations(1), SyncCursor::default()).await;

        assert!(matches!(response, Err(SyncRequestError::InvalidResponse(AuthError::BadStatus(reqwest::StatusCode::UNSUPPORTED_MEDIA_TYPE, _)))));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        server.abort();
    }

    #[tokio::test]
    async fn sync_client_rejects_a_successful_json_response() {
        let app = Router::new().route(
            "/api/sync/exchange",
            post(|| async {
                Json(SyncExchangeResponse { push: PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() }, pull: PullStateResponse { mutations: Vec::new(), next_cursor: SyncCursor::default(), has_more: false } })
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let response = exchange_changes(&reqwest::Client::new(), &sync_session(address), test_support::fixture_library_id("library"), test_support::fixture_replica_id("replica"), Vec::new(), SyncCursor::default()).await;

        assert!(matches!(response, Err(SyncRequestError::InvalidResponse(AuthError::InvalidData(message))) if message.contains("unsupported API response content type: application/json")));
        server.abort();
    }
}
