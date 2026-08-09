use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use axum::Router;
use bokheim_account_contract::{AuthResponse, EmailRequest, LoginRequest, OneTimeTokenRequest, OtherSessionsRevocationResponse, PublicRegistrationRequest, RefreshRequest, ResetPasswordRequest, SessionRevocationResponse, SessionSummary};
use server_account::{AccountEmailKind, AccountPolicy, ClientAddressSource, PostgresAccountService};
use std::sync::Arc;
use tower::ServiceExt;

async fn post<T: serde::Serialize + ?Sized>(app: &Router, path: &str, body: &T, bearer: Option<&str>) -> axum::response::Response {
    let encoded = bokheim_wire::encode(body).unwrap();
    let mut request = Request::builder().method("POST").uri(path).header(header::CONTENT_TYPE, bokheim_wire::MEDIA_TYPE).header(header::ACCEPT, bokheim_wire::MEDIA_TYPE).header("x-forwarded-for", "203.0.113.20");
    if let Some(token) = bearer {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    app.clone().oneshot(request.body(Body::from(encoded)).unwrap()).await.unwrap()
}

async fn auth_response(response: axum::response::Response) -> AuthResponse {
    assert!(response.status().is_success());
    bokheim_wire::decode(&to_bytes(response.into_body(), 64 * 1024).await.unwrap(), 64 * 1024).unwrap()
}

async fn response_body<T: serde::de::DeserializeOwned>(response: axum::response::Response) -> T {
    assert!(response.status().is_success());
    bokheim_wire::decode(&to_bytes(response.into_body(), 64 * 1024).await.unwrap(), 64 * 1024).unwrap()
}

fn accepted(request: axum::http::request::Builder) -> axum::http::request::Builder {
    request.header(header::ACCEPT, bokheim_wire::MEDIA_TYPE)
}

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn public_registration_verification_and_reset_are_end_to_end_safe() {
    let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("run through Backend/tests/run_sync_e2e.sh");
    let database = server_database::connect(&database_url).await.unwrap();
    server_database::migrate(&database).await.unwrap();
    let policy = AccountPolicy::with_blocked_email_domains(["disposable.example".to_string()]).unwrap();
    let accounts = Arc::new(PostgresAccountService::with_policy(database.clone(), policy));
    let disk = tempfile::tempdir().unwrap();
    let app = sync_server::app(
        database,
        accounts.clone(),
        sync_server::AppConfig { public_registration: true, books: disk.path().join("books"), thumbnails: disk.path().join("thumbnails"), client_address_source: ClientAddressSource::TrustedForwardedFor, ..Default::default() },
    )
    .await
    .unwrap();
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let email = format!("public-{unique}@example.com");
    let original_password = "correct-horse-public-password";

    let rejected = post(&app, "/api/auth/register", &PublicRegistrationRequest { email: "reader@disposable.example".to_owned(), password: original_password.to_owned() }, None).await;
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);

    let registration = PublicRegistrationRequest { email: email.clone(), password: original_password.to_owned() };
    let registered = post(&app, "/api/auth/register", &registration, None).await;
    assert_eq!(registered.status(), StatusCode::ACCEPTED);
    let verification = accounts.claim_email("registration-test").await.unwrap().unwrap();
    assert_eq!(verification.kind, AccountEmailKind::VerifyEmail);
    assert_eq!(verification.recipient, email);

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
    assert_eq!(app.oneshot(Request::get("/api/auth/me").header(header::AUTHORIZATION, format!("Bearer {}", current.token)).body(Body::empty()).unwrap()).await.unwrap().status(), StatusCode::UNAUTHORIZED);
}
