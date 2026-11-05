//! HTTP adapter for account creation, credentials, and sessions.

use std::time::Duration;
use crate::core::IssuedSession;
use crate::core::AuthAttempt;
use crate::postgres::PostgresAccountService;
use account_contract::{
    AuthResponse, DeleteAccountRequest, EmailRequest, EmailVerificationRequest, LoginRequest, MeResponse, OtherSessionsRevocationResponse, PublicRegistrationRequest, RefreshRequest, ResetPasswordRequest, SessionRevocationResponse, SessionSummary,
};
use axum::async_trait;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, Path, State};
use axum::http::{
    request::Parts,
    HeaderMap, HeaderName, StatusCode,
};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

const MAX_AUTH_BODY_BYTES: usize = 16 * 1024;
const GENERIC_ACCOUNT_RESPONSE_FLOOR: Duration = Duration::from_millis(300);
const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");

pub fn auth_error(error: crate::AuthError) -> StatusCode {
    match error {
        crate::AuthError::InvalidCredentials | crate::AuthError::InvalidOneTimeToken => StatusCode::UNAUTHORIZED,
        crate::AuthError::EmailTaken => StatusCode::CONFLICT,
        crate::AuthError::InvalidPassword { .. } | crate::AuthError::InvalidEmail => StatusCode::BAD_REQUEST,
        crate::AuthError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        crate::AuthError::Internal(reason) => {
            tracing::error!(%reason, "authentication error");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientAddressSource {
    DirectPeer,
    TrustedForwardedFor,
}

#[derive(Clone)]
struct AccountState {
    accounts: Arc<PostgresAccountService>,
    client_address_source: ClientAddressSource,
}

pub fn router(accounts: Arc<PostgresAccountService>, client_address_source: ClientAddressSource) -> Router {
    let state = AccountState { accounts, client_address_source };
    Router::new()
        .route("/api/auth/login", post(post_login).layer(DefaultBodyLimit::max(MAX_AUTH_BODY_BYTES)))
        .route("/api/auth/delete-account", post(post_delete_account).layer(DefaultBodyLimit::max(MAX_AUTH_BODY_BYTES)))
        .route("/api/auth/refresh", post(post_refresh).layer(DefaultBodyLimit::max(MAX_AUTH_BODY_BYTES)))
        .route("/api/auth/logout", post(post_logout))
        .route("/api/auth/me", get(get_me))
        .route("/api/auth/sessions", get(get_sessions).delete(delete_other_sessions))
        .route("/api/auth/sessions/:session_id", axum::routing::delete(delete_session))
        .route("/api/auth/register", post(post_public_registration).layer(DefaultBodyLimit::max(MAX_AUTH_BODY_BYTES)))
        .route("/api/auth/resend-verification", post(post_resend_verification).layer(DefaultBodyLimit::max(MAX_AUTH_BODY_BYTES)))
        .route("/api/auth/verify-email", post(post_verify_email).layer(DefaultBodyLimit::max(MAX_AUTH_BODY_BYTES)))
        .route("/api/auth/request-password-reset", post(post_request_password_reset).layer(DefaultBodyLimit::max(MAX_AUTH_BODY_BYTES)))
        .route("/api/auth/reset-password", post(post_reset_password).layer(DefaultBodyLimit::max(MAX_AUTH_BODY_BYTES)))
        .with_state(state)
}

struct AccountUser {
    user_id: String,
    email: String,
}

#[async_trait]
impl FromRequestParts<AccountState> for AccountUser {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &AccountState) -> Result<Self, Self::Rejection> {
        let token = bearer_token(&parts.headers)?;
        let user = state.accounts.resolve_token(token).await.map_err(auth_error)?;
        Ok(Self { user_id: user.user_id, email: user.email })
    }
}

fn bearer_token(headers: &HeaderMap) -> Result<&str, StatusCode> {
    headers.get(axum::http::header::AUTHORIZATION).and_then(|value| value.to_str().ok()).and_then(|value| value.strip_prefix("Bearer ")).filter(|token| !token.is_empty()).ok_or(StatusCode::UNAUTHORIZED)
}

pub fn client_address(source: ClientAddressSource, headers: &HeaderMap, remote: Option<ConnectInfo<SocketAddr>>) -> Result<IpAddr, StatusCode> {
    match source {
        ClientAddressSource::DirectPeer => remote.map(|ConnectInfo(address)| address.ip()).ok_or(StatusCode::INTERNAL_SERVER_ERROR),
        ClientAddressSource::TrustedForwardedFor => {
            let mut values = headers.get_all(X_FORWARDED_FOR).iter();
            let value = values.next().ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
            if values.next().is_some() {
                return Err(StatusCode::BAD_REQUEST);
            }
            let value = value.to_str().map_err(|_| StatusCode::BAD_REQUEST)?;
            if value.contains(',') {
                return Err(StatusCode::BAD_REQUEST);
            }
            value.trim().parse().map_err(|_| StatusCode::BAD_REQUEST)
        }
    }
}

async fn post_login(State(state): State<AccountState>, headers: HeaderMap, remote: Option<ConnectInfo<SocketAddr>>, body: Bytes) -> Result<Response, StatusCode> {
    let request: LoginRequest = server_wire_http::decode_request(&headers, &body, MAX_AUTH_BODY_BYTES)?;
    let address = client_address(state.client_address_source, &headers, remote)?.to_string();
    state.accounts.enforce_rate_limit(AuthAttempt::Login, &address, &request.email).await.map_err(auth_error)?;
    let issued = state.accounts.login(&request.email, &request.password).await.map_err(auth_error)?;
    state.accounts.clear_subject_rate_limit(AuthAttempt::Login, &request.email).await.map_err(auth_error)?;
    server_wire_http::response(&headers, auth_response(issued))
}

// This standalone website form uses JSON rather than the app's binary wire
// format. It requires fresh credentials and never accepts ambient cookies.
async fn post_delete_account(
    State(state): State<AccountState>, headers: HeaderMap, remote: Option<ConnectInfo<SocketAddr>>, Json(request): Json<DeleteAccountRequest>,
) -> Result<StatusCode, StatusCode> {
    if request.confirmation != "DELETE" {
        return Err(StatusCode::BAD_REQUEST);
    }
    let address = client_address(state.client_address_source, &headers, remote)?.to_string();
    state.accounts.enforce_rate_limit(AuthAttempt::DeleteAccount, &address, &request.email).await.map_err(auth_error)?;
    state.accounts.delete_account(&request.email, &request.password).await.map_err(auth_error)?;
    Ok(StatusCode::NO_CONTENT)
}


async fn post_logout(State(state): State<AccountState>, headers: HeaderMap, body: Bytes) -> Result<StatusCode, StatusCode> {
    let token = match server_wire_http::decode_optional_request::<RefreshRequest>(&headers, &body, MAX_AUTH_BODY_BYTES)? {
        Some(request) => request.refresh_token,
        None => bearer_token(&headers)?.to_owned(),
    };
    state.accounts.logout(&token).await.map_err(auth_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn post_refresh(State(state): State<AccountState>, headers: HeaderMap, body: Bytes) -> Result<Response, StatusCode> {
    let request: RefreshRequest = server_wire_http::decode_request(&headers, &body, MAX_AUTH_BODY_BYTES)?;
    let issued = state.accounts.refresh_session(&request.refresh_token).await.map_err(auth_error)?;
    server_wire_http::response(&headers, auth_response(issued))
}




async fn post_public_registration(State(state): State<AccountState>, headers: HeaderMap, remote: Option<ConnectInfo<SocketAddr>>, body: Bytes) -> Result<StatusCode, StatusCode> {
    let request: PublicRegistrationRequest = server_wire_http::decode_request(&headers, &body, MAX_AUTH_BODY_BYTES)?;
    let started = tokio::time::Instant::now();
    let address = client_address(state.client_address_source, &headers, remote)?.to_string();
    state.accounts.enforce_rate_limit(AuthAttempt::PublicRegistration, &address, &request.email).await.map_err(auth_error)?;
    state.accounts.request_public_registration(&request.email, &request.password).await.map_err(auth_error)?;
    tokio::time::sleep_until(started + GENERIC_ACCOUNT_RESPONSE_FLOOR).await;
    Ok(StatusCode::ACCEPTED)
}

async fn post_resend_verification(State(state): State<AccountState>, headers: HeaderMap, remote: Option<ConnectInfo<SocketAddr>>, body: Bytes) -> Result<StatusCode, StatusCode> {
    let request: EmailRequest = server_wire_http::decode_request(&headers, &body, MAX_AUTH_BODY_BYTES)?;
    let started = tokio::time::Instant::now();
    let address = client_address(state.client_address_source, &headers, remote)?.to_string();
    state.accounts.enforce_rate_limit(AuthAttempt::ResendVerification, &address, &request.email).await.map_err(auth_error)?;
    state.accounts.resend_verification(&request.email).await.map_err(auth_error)?;
    tokio::time::sleep_until(started + GENERIC_ACCOUNT_RESPONSE_FLOOR).await;
    Ok(StatusCode::ACCEPTED)
}

async fn post_verify_email(State(state): State<AccountState>, headers: HeaderMap, remote: Option<ConnectInfo<SocketAddr>>, body: Bytes) -> Result<Response, StatusCode> {
    let request: EmailVerificationRequest = server_wire_http::decode_request(&headers, &body, MAX_AUTH_BODY_BYTES)?;
    let address = client_address(state.client_address_source, &headers, remote)?.to_string();
    state.accounts.enforce_rate_limit(AuthAttempt::VerifyEmail, &address, &request.email).await.map_err(auth_error)?;
    let issued = state.accounts.verify_email(&request.email, &request.pin, &request.password).await.map_err(auth_error)?;
    state.accounts.clear_subject_rate_limit(AuthAttempt::VerifyEmail, &request.email).await.map_err(auth_error)?;
    server_wire_http::response(&headers, auth_response(issued))
}


fn auth_response(issued: IssuedSession) -> AuthResponse {
    AuthResponse { token: issued.token, expires_at: issued.expires_at.timestamp(), refresh_token: issued.refresh_token, refresh_expires_at: issued.refresh_expires_at.timestamp(), user_id: issued.user.user_id, email: issued.user.email }
}

async fn post_request_password_reset(State(state): State<AccountState>, headers: HeaderMap, remote: Option<ConnectInfo<SocketAddr>>, body: Bytes) -> Result<StatusCode, StatusCode> {
    let request: EmailRequest = server_wire_http::decode_request(&headers, &body, MAX_AUTH_BODY_BYTES)?;
    let started = tokio::time::Instant::now();
    let address = client_address(state.client_address_source, &headers, remote)?.to_string();
    state.accounts.enforce_rate_limit(AuthAttempt::RequestPasswordReset, &address, &request.email).await.map_err(auth_error)?;
    state.accounts.request_password_reset(&request.email).await.map_err(auth_error)?;
    tokio::time::sleep_until(started + GENERIC_ACCOUNT_RESPONSE_FLOOR).await;
    Ok(StatusCode::ACCEPTED)
}

async fn post_reset_password(State(state): State<AccountState>, headers: HeaderMap, remote: Option<ConnectInfo<SocketAddr>>, body: Bytes) -> Result<StatusCode, StatusCode> {
    let request: ResetPasswordRequest = server_wire_http::decode_request(&headers, &body, MAX_AUTH_BODY_BYTES)?;
    let address = client_address(state.client_address_source, &headers, remote)?.to_string();
    state.accounts.enforce_rate_limit(AuthAttempt::ResetPassword, &address, "password-reset").await.map_err(auth_error)?;
    state.accounts.reset_password(&request.token, &request.new_password).await.map_err(auth_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_me(headers: HeaderMap, user: AccountUser) -> Result<Response, StatusCode> {
    server_wire_http::response(&headers, MeResponse { user_id: user.user_id, email: user.email })
}

async fn get_sessions(State(state): State<AccountState>, headers: HeaderMap) -> Result<Response, StatusCode> {
    let sessions = state.accounts.list_sessions(bearer_token(&headers)?).await.map_err(auth_error)?;
    server_wire_http::response(
        &headers,
        sessions.into_iter().map(|session| SessionSummary { session_id: session.session_id, created_at: session.created_at.timestamp(), expires_at: session.expires_at.timestamp(), current: session.current }).collect::<Vec<_>>(),
    )
}

async fn delete_session(State(state): State<AccountState>, headers: HeaderMap, Path(session_id): Path<String>) -> Result<Response, StatusCode> {
    if session_id.is_empty() || session_id.len() > 64 || !session_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let revocation = state.accounts.revoke_session(bearer_token(&headers)?, &session_id).await.map_err(auth_error)?;
    server_wire_http::response(&headers, SessionRevocationResponse { revoked: revocation.revoked, signed_out: revocation.signed_out })
}

async fn delete_other_sessions(State(state): State<AccountState>, headers: HeaderMap) -> Result<Response, StatusCode> {
    let revoked = state.accounts.revoke_other_sessions(bearer_token(&headers)?).await.map_err(auth_error)?;
    server_wire_http::response(&headers, OtherSessionsRevocationResponse { revoked })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[tokio::test]
    async fn retired_web_auth_routes_return_not_found_and_cookies_are_not_credentials() {
        use axum::body::Body;
        use axum::http::{Method, Request};
        use tower::ServiceExt;
        use crate::postgres::AccountTokenKey;
        use server_postgres::PostgresDatabase;
        use sqlx::postgres::PgPoolOptions;

        // Missing routes and credentials must fail without opening a database connection.
        let pool = PgPoolOptions::new().connect_lazy("postgres://localhost/unused").unwrap();
        let accounts = Arc::new(PostgresAccountService::new(PostgresDatabase::from_pool(pool), AccountTokenKey::from_base64("WlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlo=").unwrap()));
        let app = router(accounts, ClientAddressSource::DirectPeer);
        for (method, suffix) in [(Method::POST, "login"), (Method::POST, "refresh"), (Method::POST, "logout"), (Method::GET, "me"), (Method::POST, "verify-email")] {
            let response = app.clone().oneshot(Request::builder().method(method).uri(format!("/api/auth/web/{suffix}")).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "retired route {suffix}");
        }
        let response = app.oneshot(Request::builder().uri("/api/auth/me").header(axum::http::header::COOKIE, "__Host-bokheim_access=old-token").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn direct_peer_addresses_ignore_forwarded_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(X_FORWARDED_FOR, HeaderValue::from_static("203.0.113.9"));
        let peer = "127.0.0.1:1234".parse().unwrap();
        assert_eq!(client_address(ClientAddressSource::DirectPeer, &headers, Some(ConnectInfo(peer))).unwrap(), "127.0.0.1".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn trusted_proxy_addresses_require_one_canonical_ip() {
        let mut headers = HeaderMap::new();
        headers.insert(X_FORWARDED_FOR, HeaderValue::from_static("203.0.113.9"));
        assert_eq!(client_address(ClientAddressSource::TrustedForwardedFor, &headers, None).unwrap(), "203.0.113.9".parse::<IpAddr>().unwrap());
        headers.insert(X_FORWARDED_FOR, HeaderValue::from_static("203.0.113.9, 127.0.0.1"));
        assert_eq!(client_address(ClientAddressSource::TrustedForwardedFor, &headers, None), Err(StatusCode::BAD_REQUEST));
    }


}
