use crate::ServerUrl;
use bokheim_account_contract::{AuthResponse, EmailRequest, LoginRequest, MeResponse, OneTimeTokenRequest, PublicRegistrationRequest, RefreshRequest, ResetPasswordRequest};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use sync_common::api::assets::{BlobManifestEntry, BlobManifestRequest, BlobManifestResponse};
use sync_common::api::libraries::{LibrarySummary, UpsertLibraryRequest};
use sync_common::{ClientId, LibraryId, LibraryRevision, SyncExchangeRequest, SyncExchangeResponse};

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

impl AuthError {
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::BadStatus(status, _) if *status == reqwest::StatusCode::UNAUTHORIZED)
    }

    pub fn is_authentication_failure(&self) -> bool {
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
    if value.is_empty() || value.len() > bokheim_account_contract::EMAIL_MAX_BYTES {
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
    request.header(reqwest::header::ACCEPT, bokheim_wire::MEDIA_TYPE).header(reqwest::header::ACCEPT_ENCODING, bokheim_wire::CONTENT_ENCODING_ZSTD)
}

fn api_body<T: Serialize + ?Sized>(request: reqwest::RequestBuilder, value: &T) -> Result<reqwest::RequestBuilder, AuthError> {
    let encoded = bokheim_wire::encode_transport(value, true).map_err(|error| AuthError::InvalidData(error.to_string()))?;
    let mut request = api_headers(request).header(reqwest::header::CONTENT_TYPE, bokheim_wire::MEDIA_TYPE);
    if encoded.encoding == bokheim_wire::ContentEncoding::Zstd {
        request = request.header(reqwest::header::CONTENT_ENCODING, bokheim_wire::CONTENT_ENCODING_ZSTD);
    }
    Ok(request.body(encoded.bytes))
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

pub async fn verify_email(server_url: &ServerUrl, token: &str) -> Result<Session, AuthError> {
    let body = post_auth(server_url.endpoint("api/auth/verify-email"), &OneTimeTokenRequest { token: token.to_owned() }).await?;
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

pub async fn list_account_sessions(session: &Session) -> Result<Vec<bokheim_account_contract::SessionSummary>, AuthError> {
    receive_api(reqwest::Client::builder().build()?.get(session.server_url.endpoint("api/auth/sessions")).bearer_auth(&session.token)).await
}

pub async fn revoke_account_session(session: &Session, session_id: &str) -> Result<bokheim_account_contract::SessionRevocationResponse, AuthError> {
    validate_identity(session_id, "session id")?;
    let endpoint = format!("api/auth/sessions/{}", urlencoding::encode(session_id));
    receive_api(reqwest::Client::builder().build()?.delete(session.server_url.endpoint(&endpoint)).bearer_auth(&session.token)).await
}

pub async fn revoke_other_account_sessions(session: &Session) -> Result<bokheim_account_contract::OtherSessionsRevocationResponse, AuthError> {
    receive_api(reqwest::Client::builder().build()?.delete(session.server_url.endpoint("api/auth/sessions")).bearer_auth(&session.token)).await
}

pub async fn list_libraries(session: &Session) -> Result<Vec<LibrarySummary>, AuthError> {
    receive_api(reqwest::Client::builder().build()?.get(session.server_url.endpoint("api/libraries")).bearer_auth(&session.token)).await
}

pub async fn upsert_library(session: &Session, library_id: sync_common::LibraryId, library_name: String) -> Result<LibrarySummary, AuthError> {
    let request = UpsertLibraryRequest { library_id, library_name };
    send_api(reqwest::Client::builder().build()?.post(session.server_url.endpoint("api/libraries")).bearer_auth(&session.token), &request).await
}

pub async fn negotiate_blobs(client: &reqwest::Client, session: &Session, library_id: &LibraryId, blobs: Vec<BlobManifestEntry>) -> Result<BlobManifestResponse, AuthError> {
    let requested: HashSet<_> = blobs.iter().map(|blob| blob.content_hash.clone()).collect();
    if requested.len() != blobs.len() {
        return Err(AuthError::InvalidData("blob manifest contains duplicate hashes".to_string()));
    }
    let response: BlobManifestResponse =
        send_api(client.post(session.server_url.endpoint(&format!("api/libraries/{}/blobs/negotiate", urlencoding::encode(library_id.as_str())))).bearer_auth(&session.token), &BlobManifestRequest { blobs }).await?;
    let classified = response.owned.iter().chain(&response.claimed).chain(&response.upload).chain(response.rejected.iter().map(|entry| &entry.content_hash));
    let mut unique = HashSet::with_capacity(requested.len());
    for hash in classified {
        if !requested.contains(hash) || !unique.insert(hash.clone()) {
            return Err(AuthError::InvalidData("blob manifest response contains an unknown or repeated hash".to_string()));
        }
    }
    if unique != requested {
        return Err(AuthError::InvalidData("blob manifest response omitted a requested hash".to_string()));
    }
    Ok(response)
}

pub async fn exchange_changes(
    client: &reqwest::Client, session: &Session, library_id: LibraryId, replica_id: ClientId, mutations: Vec<bokheim_domain::StateMutation>, cursor: Option<LibraryRevision>,
) -> Result<SyncExchangeResponse, AuthError> {
    let mutations = mutations.iter().map(bokheim_domain::StateMutation::to_wire).collect::<Result<Vec<_>, _>>().map_err(|error| AuthError::InvalidData(error.to_string()))?;
    let request = SyncExchangeRequest { library_id, replica_id, mutations, cursor };
    post_api(client, session, "api/sync/exchange", &request).await
}

async fn post_api<T: Serialize, R: DeserializeOwned>(client: &reqwest::Client, session: &Session, path: &str, value: &T) -> Result<R, AuthError> {
    send_api(client.post(session.server_url.endpoint(path)).bearer_auth(&session.token), value).await
}

async fn decode_api_response<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, AuthError> {
    let response = ensure_ok(response).await?;
    let content_type = response.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).unwrap_or_default().to_owned();
    let encoding = match response.headers().get(reqwest::header::CONTENT_ENCODING).and_then(|value| value.to_str().ok()).map(str::trim) {
        None | Some("") => bokheim_wire::ContentEncoding::Identity,
        Some(value) if value.eq_ignore_ascii_case("identity") => bokheim_wire::ContentEncoding::Identity,
        Some(value) if value.eq_ignore_ascii_case(bokheim_wire::CONTENT_ENCODING_ZSTD) => bokheim_wire::ContentEncoding::Zstd,
        Some(value) => return Err(AuthError::InvalidData(format!("unsupported API response content encoding: {value}"))),
    };
    let body = response.bytes().await?;
    if !bokheim_wire::is_current_media_type(&content_type) {
        return Err(AuthError::InvalidData(format!("unsupported API response content type: {content_type}")));
    }
    bokheim_wire::decode_transport(&body, encoding, bokheim_wire::MAX_DECODED_RESPONSE_BYTES).map_err(|error| AuthError::InvalidData(error.to_string()))
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
    use sync_common::{PullStateResponse, PushMutationsResponse};
    use axum::body::Bytes;
    use axum::http::{header, HeaderMap, HeaderValue};
    use axum::response::IntoResponse;
    use axum::routing::post;
    use axum::{Json, Router};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    const TEST_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn session_deserialization_validates_every_field() {
        let valid = serde_json::json!({ "server_url": "https://example.com", "user_id": "user-1", "email": "reader@example.com", "token": TEST_TOKEN });
        let session: Session = serde_json::from_value(valid.clone()).unwrap();
        assert_eq!(session.server_url().as_str(), "https://example.com/");
        for (field, invalid) in [("server_url", "file:///tmp/server"), ("user_id", "bad/id"), ("email", &"a".repeat(bokheim_account_contract::EMAIL_MAX_BYTES + 1)), ("token", "short")] {
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
                    let encoding =
                        if headers.get(header::CONTENT_ENCODING).and_then(|value| value.to_str().ok()) == Some(bokheim_wire::CONTENT_ENCODING_ZSTD) { bokheim_wire::ContentEncoding::Zstd } else { bokheim_wire::ContentEncoding::Identity };
                    let request: RefreshRequest = bokheim_wire::decode_transport(&body, encoding, bokheim_wire::MAX_DECODED_REQUEST_BYTES).unwrap();
                    observed.store(request.refresh_token == "b".repeat(43), Ordering::SeqCst);
                    let encoded = bokheim_wire::encode(&AuthResponse {
                        token: "c".repeat(43),
                        expires_at: 1_900_000_000,
                        refresh_token: "d".repeat(43),
                        refresh_expires_at: 1_910_000_000,
                        user_id: "user-1".to_owned(),
                        email: "reader@example.com".to_owned(),
                    })
                    .unwrap();
                    let mut response = encoded.into_response();
                    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(bokheim_wire::MEDIA_TYPE));
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

    fn sync_mutations(count: usize) -> Vec<bokheim_domain::StateMutation> {
        (1..=count)
            .map(|sequence| bokheim_domain::StateMutation {
                mutation_id: sync_common::MutationId::new(),
                body: bokheim_domain::MutationBody::BookLifecycle {
                    book_id: sync_common::BookId::new(sequence as u64),
                    value: bokheim_domain::BookLifecycleState::Present {
                        metadata: bokheim_domain::BookMetadataState { title: format!("Book {sequence}"), contributors: Vec::new(), description: String::new(), publication: Default::default() },
                        added_at: sync_common::UnixMillis::new(1).unwrap(),
                        content_hash: sync_common::ContentHash::parse(&"a".repeat(64)).unwrap(),
                        format: bokheim_domain::BookFormat::Epub,
                    },
                },
                changed_at: sync_common::UnixMillis::new(1).unwrap(),
                replica_seq: sync_common::ReplicaSeq::new(sequence as u64).unwrap(),
            })
            .collect()
    }

    fn sync_session(address: std::net::SocketAddr) -> Session {
        Session::try_new(ServerUrl::parse(&format!("http://{address}")).unwrap(), "user-1".to_string(), "reader@example.com".to_string(), TEST_TOKEN.to_string()).unwrap()
    }

    #[tokio::test]
    async fn sync_client_sends_and_receives_versioned_compressed_binary() {
        let observed_binary = Arc::new(AtomicBool::new(false));
        let observed_compressed = Arc::new(AtomicBool::new(false));
        let binary = observed_binary.clone();
        let compressed = observed_compressed.clone();
        let app = Router::new().route(
            "/api/sync/exchange",
            post(move |headers: HeaderMap, body: Bytes| {
                let binary = binary.clone();
                let compressed = compressed.clone();
                async move {
                    let request_encoding = if headers.get(header::CONTENT_ENCODING).and_then(|value| value.to_str().ok()) == Some(sync_common::wire::CONTENT_ENCODING_ZSTD) {
                        compressed.store(true, Ordering::SeqCst);
                        sync_common::wire::ContentEncoding::Zstd
                    } else {
                        sync_common::wire::ContentEncoding::Identity
                    };
                    binary.store(headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok()).is_some_and(|value| value.starts_with(sync_common::wire::MEDIA_TYPE_BASE)), Ordering::SeqCst);
                    let request: SyncExchangeRequest = sync_common::wire::decode_transport(&body, request_encoding, sync_common::wire::MAX_DECODED_REQUEST_BYTES).unwrap();
                    let response = SyncExchangeResponse {
                        push: PushMutationsResponse { accepted: request.mutations.iter().map(|mutation| mutation.mutation_id).collect(), rejected: Vec::new() },
                        pull: PullStateResponse { mutations: Vec::new(), next_cursor: None, has_more: false },
                    };
                    let encoded = sync_common::wire::encode_transport(&response, true).unwrap();
                    let mut response = encoded.bytes.into_response();
                    response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(sync_common::wire::MEDIA_TYPE));
                    if encoded.encoding == sync_common::wire::ContentEncoding::Zstd {
                        response.headers_mut().insert(header::CONTENT_ENCODING, HeaderValue::from_static(sync_common::wire::CONTENT_ENCODING_ZSTD));
                    }
                    response
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mutations = sync_mutations(100);

        let response = exchange_changes(&reqwest::Client::new(), &sync_session(address), LibraryId::parse("library").unwrap(), ClientId::parse("replica").unwrap(), mutations, None).await.unwrap();

        assert!(observed_binary.load(Ordering::SeqCst));
        assert!(observed_compressed.load(Ordering::SeqCst));
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

        let response = exchange_changes(&reqwest::Client::new(), &sync_session(address), LibraryId::parse("library").unwrap(), ClientId::parse("replica").unwrap(), sync_mutations(1), None).await;

        assert!(matches!(response, Err(AuthError::BadStatus(reqwest::StatusCode::UNSUPPORTED_MEDIA_TYPE, _))));
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
        server.abort();
    }

    #[tokio::test]
    async fn sync_client_rejects_a_successful_json_response() {
        let app = Router::new().route(
            "/api/sync/exchange",
            post(|| async { Json(SyncExchangeResponse { push: PushMutationsResponse { accepted: Vec::new(), rejected: Vec::new() }, pull: PullStateResponse { mutations: Vec::new(), next_cursor: None, has_more: false } }) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let response = exchange_changes(&reqwest::Client::new(), &sync_session(address), LibraryId::parse("library").unwrap(), ClientId::parse("replica").unwrap(), Vec::new(), None).await;

        assert!(matches!(response, Err(AuthError::InvalidData(message)) if message.contains("unsupported API response content type: application/json")));
        server.abort();
    }
}
