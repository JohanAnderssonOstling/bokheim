use crate::http::{decode_response, post_empty, request, send, with_body};
use crate::session::{AuthError, Session};
use crate::Url;
use account_contract::{AuthResponse, DeleteAccountRequest, EmailRequest, EmailVerificationRequest, LoginRequest, PublicRegistrationRequest, RefreshRequest, ResetPasswordRequest};
use reqwest::{Method, StatusCode};
use serde::Serialize;

async fn post_auth(server_url: &Url, endpoint: &str, body: &impl Serialize) -> Result<Session, AuthError> {
    let response: AuthResponse = decode_response(with_body(request(server_url, Method::POST, endpoint)?, body)?).await?;
    Session::try_new_with_tokens(server_url.clone(), response.user_id, response.email, response.token, Some(response.expires_at), Some(response.refresh_token), Some(response.refresh_expires_at))
}

pub async fn login(server_url: &Url, email: &str, password: &str) -> Result<Session, AuthError> {
    post_auth(server_url, "api/auth/login", &LoginRequest { email: email.to_owned(), password: password.to_owned() }).await
}

pub async fn request_public_registration(server_url: &Url, email: &str, password: &str) -> Result<(), AuthError> {
    post_empty(server_url, "api/auth/register", &PublicRegistrationRequest { email: email.to_owned(), password: password.to_owned() }).await
}

pub async fn resend_verification(server_url: &Url, email: &str) -> Result<(), AuthError> {
    post_empty(server_url, "api/auth/resend-verification", &EmailRequest { email: email.to_owned() }).await
}

pub async fn verify_email(server_url: &Url, email: &str, pin: &str, password: &str) -> Result<Session, AuthError> {
    post_auth(server_url, "api/auth/verify-email", &EmailVerificationRequest { email: email.to_owned(), pin: pin.to_owned(), password: password.to_owned() }).await
}

pub async fn request_password_reset(server_url: &Url, email: &str) -> Result<(), AuthError> {
    post_empty(server_url, "api/auth/request-password-reset", &EmailRequest { email: email.to_owned() }).await
}

pub async fn reset_password(server_url: &Url, token: &str, new_password: &str) -> Result<(), AuthError> {
    post_empty(server_url, "api/auth/reset-password", &ResetPasswordRequest { token: token.to_owned(), new_password: new_password.to_owned() }).await
}

pub async fn logout(session: &Session) -> Result<(), AuthError> {
    let refresh_token = session.refresh_token().ok_or(AuthError::SignInRequired("session has no refresh token"))?;
    post_empty(session.server_url(), "api/auth/logout", &RefreshRequest { refresh_token: refresh_token.to_owned() }).await
}

/// Deletes the account only after checking its current password again.
pub async fn delete_account(session: &Session, password: &str) -> Result<(), AuthError> {
    let body = DeleteAccountRequest { email: session.email().to_owned(), password: password.to_owned(), confirmation: "DELETE".to_owned() };
    let response = send(request(session.server_url(), Method::POST, "api/auth/delete-account")?.json(&body)).await?;
    if response.status() != StatusCode::NO_CONTENT {
        return Err(AuthError::InvalidData("account deletion was not confirmed by the server".into()));
    }
    Ok(())
}

pub async fn refresh_session(session: &Session) -> Result<Session, AuthError> {
    let refresh_token = session.refresh_token().ok_or(AuthError::SignInRequired("session has no refresh token"))?;
    if session.refresh_expires_within(0) {
        return Err(AuthError::SignInRequired("refresh token has expired"));
    }
    let refreshed = post_auth(session.server_url(), "api/auth/refresh", &RefreshRequest { refresh_token: refresh_token.to_owned() }).await?;
    if refreshed.user_id() != session.user_id() {
        return Err(AuthError::InvalidData("refreshed session changed authenticated user".to_owned()));
    }
    Ok(refreshed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Bytes;
    use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
    use axum::response::{IntoResponse, Response};
    use axum::routing::{get, post};
    use axum::Router;
    use flate2::{write::GzEncoder, Compression};
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use sync_common::transport as wire;

    #[tokio::test]
    async fn deletion_requires_server_confirmation_and_preserves_authentication_errors() {
        let app = Router::new().route(
            "/api/auth/delete-account",
            post(|headers: HeaderMap, axum::Json(body): axum::Json<DeleteAccountRequest>| async move {
                assert_eq!(body.email, "reader@example.com");
                assert_eq!(body.confirmation, "DELETE");
                assert!(!headers.contains_key(header::AUTHORIZATION), "fresh password authorizes deletion");
                match body.password.as_str() {
                    "correct-password" => StatusCode::NO_CONTENT,
                    "wrong-password" => StatusCode::UNAUTHORIZED,
                    _ => StatusCode::OK,
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let session = Session::try_new(origin, "user-1".into(), "reader@example.com".into(), "a".repeat(43)).unwrap();
        delete_account(&session, "correct-password").await.unwrap();
        assert!(delete_account(&session, "wrong-password").await.unwrap_err().is_unauthorized());
        assert!(matches!(delete_account(&session, "unexpected-response").await, Err(AuthError::InvalidData(_))));
        server.abort();
    }
    const TEST_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn gzip_response(body: &[u8], content_type: &'static str) -> Response {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(body).unwrap();
        let mut response = encoder.finish().unwrap().into_response();
        response.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
        response.headers_mut().insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        response
    }

    #[tokio::test]
    async fn logout_sends_refresh_token_and_preserves_status_errors() {
        let app = Router::new().route(
            "/api/auth/logout",
            post(|headers: HeaderMap, body: Bytes| async move {
                assert_eq!(headers.get(header::ACCEPT).unwrap(), wire::MEDIA_TYPE);
                assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), wire::MEDIA_TYPE);
                let request: RefreshRequest = wire::decode(&body, wire::MAX_DECODED_REQUEST_BYTES).unwrap();
                match request.refresh_token.as_str() {
                    token if token == "b".repeat(43) => (StatusCode::NO_CONTENT, "").into_response(),
                    _ => (StatusCode::UNAUTHORIZED, gzip_response(b"refresh token revoked", "text/plain")).into_response(),
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let make_session =
            |refresh_token: char| Session::try_new_with_tokens(origin.clone(), "user-1".into(), "reader@example.com".into(), TEST_TOKEN.into(), Some(1_900_000_000), Some(refresh_token.to_string().repeat(43)), Some(1_910_000_000)).unwrap();

        logout(&make_session('b')).await.unwrap();
        let error = logout(&make_session('c')).await.unwrap_err();
        assert!(error.is_unauthorized());
        assert!(matches!(error, AuthError::Http(error) if error.status() == Some(StatusCode::UNAUTHORIZED)));
        server.abort();
    }

    #[tokio::test]
    async fn refresh_and_snapshot_decode_gzip_without_changing_identity() {
        let received_old_refresh = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&received_old_refresh);
        let library_id = sync_common::LibraryId::parse_str("00000000-0000-4000-8000-000000000001").unwrap();
        let expected_snapshot = account_contract::AccountSnapshotResponse {
            libraries: vec![sync_common::api::libraries::LibrarySummary { library_id, library_name: "Classics".to_owned(), storage: Default::default() }],
            deleted_library_ids: Vec::new(),
            storage: account_contract::StorageUsageResponse { used_bytes: 40, reserved_bytes: 2, quota_bytes: 100 },
            library_storage: vec![account_contract::LibraryStorageUsage { library_id: library_id.to_string(), used_bytes: 40 }],
        };
        let response_snapshot = expected_snapshot.clone();
        let app = Router::new().route(
            "/api/auth/refresh",
            post(move |headers: HeaderMap, body: Bytes| {
                let observed = Arc::clone(&observed);
                async move {
                    assert!(headers.get(header::CONTENT_ENCODING).is_none());
                    assert!(headers.get(header::ACCEPT_ENCODING).unwrap().to_str().unwrap().split(',').any(|encoding| encoding.trim() == "gzip"));
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
                    gzip_response(&encoded, wire::MEDIA_TYPE)
                }
            }),
        );
        let app = app.route(
            "/api/account/snapshot",
            get(move |headers: HeaderMap| async move {
                assert_eq!(headers.get(header::AUTHORIZATION).and_then(|value| value.to_str().ok()), Some(format!("Bearer {}", "c".repeat(43)).as_str()));
                let encoded = wire::encode(&response_snapshot).unwrap();
                gzip_response(&encoded, wire::MEDIA_TYPE)
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let session =
            Session::try_new_with_tokens(Url::parse(&format!("http://{address}")).unwrap(), "user-1".to_owned(), "reader@example.com".to_owned(), TEST_TOKEN.to_owned(), Some(1_800_000_000), Some("b".repeat(43)), Some(1_910_000_000))
                .unwrap();

        let refreshed = refresh_session(&session).await.unwrap();

        assert!(received_old_refresh.load(Ordering::SeqCst));
        assert_eq!(refreshed.token(), "c".repeat(43));
        assert_eq!(refreshed.refresh_token(), Some("d".repeat(43).as_str()));
        assert_eq!(refreshed.user_id(), session.user_id());
        assert_eq!(refreshed.email(), session.email());
        assert_eq!(crate::account::account_snapshot(&refreshed).await.unwrap(), expected_snapshot);
        server.abort();
    }
}
