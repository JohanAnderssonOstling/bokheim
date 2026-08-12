use account_contract::{
    AuthResponse, EmailRequest, LibraryStorageUsageResponse, LoginRequest, MeResponse, OneTimeTokenRequest, OtherSessionsRevocationResponse, PublicRegistrationRequest, RefreshRequest, ResetPasswordRequest, SessionRevocationResponse,
    SessionSummary, StorageUsageResponse,
};
use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use axum::Router;
use server_account::{AccountEmailKind, ClientAddressSource, PostgresAccountService};
use std::sync::Arc;
use tower::ServiceExt;

async fn post<T: serde::Serialize + ?Sized>(app: &Router, path: &str, body: &T, bearer: Option<&str>) -> axum::response::Response {
    let encoded = wire::encode(body).unwrap();
    let mut request = Request::builder().method("POST").uri(path).header(header::CONTENT_TYPE, wire::MEDIA_TYPE).header(header::ACCEPT, wire::MEDIA_TYPE).header("x-forwarded-for", "203.0.113.20");
    if let Some(token) = bearer {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    app.clone().oneshot(request.body(Body::from(encoded)).unwrap()).await.unwrap()
}

async fn auth_response(response: axum::response::Response) -> AuthResponse {
    assert!(response.status().is_success());
    wire::decode(&to_bytes(response.into_body(), 64 * 1024).await.unwrap(), 64 * 1024).unwrap()
}

async fn response_body<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
    assert!(response.status().is_success());
    wire::decode(&to_bytes(response.into_body(), 64 * 1024).await.unwrap(), 64 * 1024).unwrap()
}

fn session_cookie(response: &axum::response::Response) -> String {
    response.headers().get_all(header::SET_COOKIE).iter().map(|value| value.to_str().unwrap().split(';').next().unwrap()).collect::<Vec<_>>().join("; ")
}

fn accepted(request: axum::http::request::Builder) -> axum::http::request::Builder {
    request.header(header::ACCEPT, wire::MEDIA_TYPE)
}

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn public_registration_verification_and_reset_are_end_to_end_safe() {
    let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("run through Backend/tests/run_sync_e2e.sh");
    let database = server_postgres::connect(&database_url).await.unwrap();
    server_postgres::migrate(&database).await.unwrap();
    let accounts = Arc::new(PostgresAccountService::new(database.clone()));
    let disk = tempfile::tempdir().unwrap();
    let app = sync_server::app(
        database,
        accounts.clone(),
        sync_server::AppConfig { books: disk.path().join("books"), thumbnails: disk.path().join("thumbnails"), client_address_source: ClientAddressSource::TrustedForwardedFor, ..Default::default() },
    )
    .await
    .unwrap();
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let email = format!("public-{unique}@example.com");
    let original_password = "correct-horse-public-password";

    let registration = PublicRegistrationRequest { email: email.clone(), password: original_password.to_owned() };
    let registered = post(&app, "/api/auth/register", &registration, None).await;
    assert_eq!(registered.status(), StatusCode::ACCEPTED);
    let verification = accounts.claim_email("registration-test").await.unwrap().unwrap();
    assert_eq!(verification.kind, AccountEmailKind::VerifyEmail);
    assert_eq!(verification.recipient, email);
    assert_eq!(verification.token.len(), 6);
    assert!(verification.token.bytes().all(|byte| byte.is_ascii_digit()));

    // Existing account identifiers receive the same generic response and do
    // not create another pending account or leak an email.
    let verification_request = OneTimeTokenRequest { token: verification.token };
    let verified = auth_response(post(&app, "/api/auth/verify-email", &verification_request, None).await).await;
    assert_eq!(verified.email, email);
    assert_eq!(post(&app, "/api/auth/verify-email", &verification_request, None).await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(post(&app, "/api/auth/register", &registration, None).await.status(), StatusCode::ACCEPTED);
    assert!(accounts.claim_email("registration-test").await.unwrap().is_none());
    assert_eq!(app.clone().oneshot(accepted(Request::get("/api/auth/me")).header(header::AUTHORIZATION, format!("Bearer {}", verified.token)).body(Body::empty()).unwrap()).await.unwrap().status(), StatusCode::OK);

    assert_eq!(post(&app, "/api/auth/request-password-reset", &EmailRequest { email: "unknown@example.com".to_owned() }, None).await.status(), StatusCode::ACCEPTED);
    assert_eq!(post(&app, "/api/auth/request-password-reset", &EmailRequest { email: email.clone() }, None).await.status(), StatusCode::ACCEPTED);
    let reset = accounts.claim_email("registration-test").await.unwrap().unwrap();
    assert_eq!(reset.kind, AccountEmailKind::ResetPassword);
    let new_password = "a-different-correct-horse-password";
    let reset_request = ResetPasswordRequest { token: reset.token, new_password: new_password.to_owned() };
    assert_eq!(post(&app, "/api/auth/reset-password", &reset_request, None).await.status(), StatusCode::NO_CONTENT);

    // Reset is single-use and revokes every pre-reset session.
    assert_eq!(post(&app, "/api/auth/reset-password", &reset_request, None).await.status(), StatusCode::UNAUTHORIZED);
    let old_session = app.clone().oneshot(Request::builder().uri("/api/auth/me").header(header::AUTHORIZATION, format!("Bearer {}", verified.token)).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(old_session.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(post(&app, "/api/auth/refresh", &RefreshRequest { refresh_token: verified.refresh_token.clone() }, None).await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(post(&app, "/api/auth/login", &LoginRequest { email: email.clone(), password: original_password.to_owned() }, None).await.status(), StatusCode::UNAUTHORIZED);
    let valid_login = LoginRequest { email: email.clone(), password: new_password.to_owned() };

    let web_login = app
        .clone()
        .oneshot(
            Request::post("/api/auth/web/login")
                .header(header::CONTENT_TYPE, wire::MEDIA_TYPE)
                .header(header::ACCEPT, wire::MEDIA_TYPE)
                .header("x-forwarded-for", "203.0.113.21")
                .header("sec-fetch-site", "same-origin")
                .body(Body::from(wire::encode(&valid_login).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(web_login.status(), StatusCode::OK);
    let cookies = session_cookie(&web_login);
    assert!(cookies.contains("__Host-bokheim_access="));
    assert!(cookies.contains("__Host-bokheim_refresh="));
    let web_profile: MeResponse = response_body(web_login).await;
    assert_eq!(web_profile.email, email);
    let restored = app.clone().oneshot(accepted(Request::get("/api/auth/web/me")).header(header::COOKIE, &cookies).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(response_body::<MeResponse>(restored).await.email, email);
    let web_logout = app.clone().oneshot(accepted(Request::post("/api/auth/web/logout")).header(header::COOKIE, &cookies).header("sec-fetch-site", "same-origin").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(web_logout.status(), StatusCode::NO_CONTENT);
    assert_eq!(web_logout.headers().get_all(header::SET_COOKIE).iter().filter(|value| value.to_str().unwrap().contains("Max-Age=0")).count(), 2);
    assert_eq!(app.clone().oneshot(accepted(Request::get("/api/auth/web/me")).header(header::COOKIE, &cookies).body(Body::empty()).unwrap()).await.unwrap().status(), StatusCode::UNAUTHORIZED);

    let current = auth_response(post(&app, "/api/auth/login", &valid_login, None).await).await;
    let other = auth_response(post(&app, "/api/auth/login", &valid_login, None).await).await;
    let sessions_response = app.clone().oneshot(accepted(Request::get("/api/auth/sessions")).header(header::AUTHORIZATION, format!("Bearer {}", current.token)).body(Body::empty()).unwrap()).await.unwrap();
    let sessions: Vec<SessionSummary> = response_body(sessions_response).await;
    assert_eq!(sessions.len(), 2);
    let current_session = sessions.iter().find(|session| session.current).unwrap();
    let other_session = sessions.iter().find(|session| !session.current).unwrap();
    assert_eq!(app.clone().oneshot(accepted(Request::get("/api/auth/me")).header(header::AUTHORIZATION, format!("Bearer {}", other.token)).body(Body::empty()).unwrap()).await.unwrap().status(), StatusCode::OK);

    let revoked: SessionRevocationResponse = response_body(
        app.clone().oneshot(accepted(Request::delete(format!("/api/auth/sessions/{}", other_session.session_id))).header(header::AUTHORIZATION, format!("Bearer {}", current.token)).body(Body::empty()).unwrap()).await.unwrap(),
    )
    .await;
    assert!(revoked.revoked);
    assert!(!revoked.signed_out);
    assert_eq!(app.clone().oneshot(Request::get("/api/auth/me").header(header::AUTHORIZATION, format!("Bearer {}", other.token)).body(Body::empty()).unwrap()).await.unwrap().status(), StatusCode::UNAUTHORIZED);

    let third = auth_response(post(&app, "/api/auth/login", &valid_login, None).await).await;
    assert_eq!(app.clone().oneshot(accepted(Request::get("/api/auth/me")).header(header::AUTHORIZATION, format!("Bearer {}", third.token)).body(Body::empty()).unwrap()).await.unwrap().status(), StatusCode::OK);
    let others: OtherSessionsRevocationResponse =
        response_body(app.clone().oneshot(accepted(Request::delete("/api/auth/sessions")).header(header::AUTHORIZATION, format!("Bearer {}", current.token)).body(Body::empty()).unwrap()).await.unwrap()).await;
    assert_eq!(others.revoked, 1);
    assert_eq!(app.clone().oneshot(Request::get("/api/auth/me").header(header::AUTHORIZATION, format!("Bearer {}", third.token)).body(Body::empty()).unwrap()).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    assert_eq!(app.clone().oneshot(accepted(Request::get("/api/auth/me")).header(header::AUTHORIZATION, format!("Bearer {}", current.token)).body(Body::empty()).unwrap()).await.unwrap().status(), StatusCode::OK);

    let signed_out: SessionRevocationResponse = response_body(
        app.clone().oneshot(accepted(Request::delete(format!("/api/auth/sessions/{}", current_session.session_id))).header(header::AUTHORIZATION, format!("Bearer {}", current.token)).body(Body::empty()).unwrap()).await.unwrap(),
    )
    .await;
    assert!(signed_out.signed_out);
    assert_eq!(app.clone().oneshot(Request::get("/api/auth/me").header(header::AUTHORIZATION, format!("Bearer {}", current.token)).body(Body::empty()).unwrap()).await.unwrap().status(), StatusCode::UNAUTHORIZED);

    let configured_email = format!("configured-{unique}@example.com");
    let configured = accounts.ensure_configured_user(&configured_email, original_password).await.unwrap();
    let configured_session = accounts.login(&configured_email, original_password).await.unwrap();
    let same = accounts.ensure_configured_user(&configured_email, original_password).await.unwrap();
    assert_eq!(same.user_id, configured.user_id);
    assert_eq!(accounts.resolve_token(&configured_session.token).await.unwrap().user_id, configured.user_id);

    let database = server_postgres::connect(&database_url).await.unwrap();
    let quota = 1_000_i64 * 1_000 * 1_000 * 1_000;
    assert_eq!(server_postgres::configure_user_storage_quota(&database, &configured.user_id, quota).await.unwrap(), quota);
    let storage: StorageUsageResponse =
        response_body(app.clone().oneshot(accepted(Request::get("/api/account/storage")).header(header::AUTHORIZATION, format!("Bearer {}", configured_session.token)).body(Body::empty()).unwrap()).await.unwrap()).await;
    assert_eq!(storage, StorageUsageResponse { used_bytes: 0, reserved_bytes: 0, quota_bytes: quota as u64 });
    // An account holding nothing reports no library shares at all rather than a
    // row of zeroes: the client knows which libraries exist and fills the rest.
    let library_storage: LibraryStorageUsageResponse =
        response_body(app.clone().oneshot(accepted(Request::get("/api/account/storage/libraries")).header(header::AUTHORIZATION, format!("Bearer {}", configured_session.token)).body(Body::empty()).unwrap()).await.unwrap()).await;
    assert_eq!(library_storage, LibraryStorageUsageResponse { libraries: Vec::new() });

    let rotated_password = "configured-user-rotated-password";
    accounts.ensure_configured_user(&configured_email, rotated_password).await.unwrap();
    assert!(accounts.resolve_token(&configured_session.token).await.is_err());
    assert!(accounts.login(&configured_email, original_password).await.is_err());
    assert_eq!(accounts.login(&configured_email, rotated_password).await.unwrap().user.user_id, configured.user_id);
}
