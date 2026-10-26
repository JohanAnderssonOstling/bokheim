use account_contract::DeleteAccountRequest;
use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::response::IntoResponse;
use axum::{Json, Router};
use server_account::{AccountTokenKey, AuthError, ClientAddressSource, PostgresAccountService};
use std::sync::Arc;
use tower::ServiceExt;
use uuid::Uuid;

async fn delete(app: &Router, email: &str, password: &str, confirmation: &str) -> StatusCode {
    let body = Json(DeleteAccountRequest { email: email.to_owned(), password: password.to_owned(), confirmation: confirmation.to_owned() }).into_response().into_body();
    app.clone().oneshot(Request::post("/api/auth/delete-account").header(header::CONTENT_TYPE, "application/json").header("x-forwarded-for", "203.0.113.25").body(body).unwrap()).await.unwrap().status()
}

#[tokio::test]
#[ignore = "requires SYNC_E2E_DATABASE_URL"]
async fn deletion_removes_the_user_and_owned_data_and_revokes_cached_sessions() {
    let database = server_postgres::connect(&std::env::var("SYNC_E2E_DATABASE_URL").unwrap()).await.unwrap();
    server_postgres::migrate(&database).await.unwrap();
    let pool = database.pool();
    let accounts = Arc::new(PostgresAccountService::new(database.clone(), AccountTokenKey::from_base64("WlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlpaWlo=").unwrap()));
    let app = server_account::http::router(accounts.clone(), ClientAddressSource::TrustedForwardedFor);
    let unique = Uuid::new_v4();
    let email = format!("delete-{unique}@example.com");
    let other_email = format!("keep-{unique}@example.com");
    let password = "delete-account-test-password";
    let user = accounts.ensure_configured_user(&email, password).await.unwrap();
    let other = accounts.ensure_configured_user(&other_email, password).await.unwrap();
    let session = accounts.login(&email, password).await.unwrap();
    let second = accounts.login(&email, password).await.unwrap();
    let other_session = accounts.login(&other_email, password).await.unwrap();
    // Warm both identities to ensure deletion invalidates every cached session.
    accounts.resolve_token(&session.token).await.unwrap();
    accounts.resolve_token(&second.token).await.unwrap();
    accounts.resolve_token(&other_session.token).await.unwrap();

    let library_id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO libraries(id, user_id, name) VALUES ($1, $2, 'Delete me')").bind(&library_id).bind(&user.user_id).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO sync_state(library_id, kind, entity_key, value, changed_at, replica_id, replica_seq, version_rank) VALUES ($1, 'reading_position', $2, $2, 1, 'test-replica', 1, 0)")
        .bind(&library_id)
        .bind(vec![1u8; 32])
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO opds_feeds(id, user_id, name, url) VALUES ($1, $2, 'Feed', 'https://example.com/opds')").bind(unique.to_string()).bind(&user.user_id).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO user_storage_account(user_id) VALUES ($1)").bind(&user.user_id).execute(pool).await.unwrap();
    accounts.request_password_reset(&email).await.unwrap();
    // Include address-keyed records, which are not removed by user foreign keys.
    sqlx::query("INSERT INTO pending_registration(id, email, password_hash, verification_hash, expires_at) VALUES ($1, $2, 'unused', 'unused', now() + INTERVAL '1 hour')").bind(unique.to_string()).bind(&email).execute(pool).await.unwrap();

    assert_eq!(delete(&app, &email, password, "").await, StatusCode::BAD_REQUEST);
    assert_eq!(delete(&app, &email, "wrong-password", "DELETE").await, StatusCode::UNAUTHORIZED);
    assert_eq!(delete(&app, &format!("unknown-{unique}@example.com"), password, "DELETE").await, StatusCode::UNAUTHORIZED);
    assert!(accounts.login(&email, password).await.is_ok(), "failed deletion must preserve the account");

    // Ambient browser credentials and ordinary HTML form posts cannot delete.
    assert_eq!(app.clone().oneshot(Request::get("/api/auth/delete-account").body(Body::empty()).unwrap()).await.unwrap().status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(app.clone().oneshot(Request::post("/api/auth/delete-account").header(header::COOKIE, "bokheim=unused").body(Body::empty()).unwrap()).await.unwrap().status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(
        app.clone().oneshot(Request::post("/api/auth/delete-account").header(header::CONTENT_TYPE, "application/x-www-form-urlencoded").body(Body::from("confirmation=DELETE")).unwrap()).await.unwrap().status(),
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    assert_eq!(app.clone().oneshot(Request::post("/api/auth/delete-account").header(header::CONTENT_TYPE, "application/json").body(Body::from("x".repeat(17 * 1024))).unwrap()).await.unwrap().status(), StatusCode::PAYLOAD_TOO_LARGE);

    assert_eq!(delete(&app, &email.to_uppercase(), password, "DELETE").await, StatusCode::NO_CONTENT);
    for table in ["users", "auth_session", "auth_token", "password_reset", "libraries", "opds_feeds", "user_storage_account"] {
        let column = if table == "users" { "id" } else { "user_id" };
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE {column}=$1")).bind(&user.user_id).fetch_one(pool).await.unwrap();
        assert_eq!(count, 0, "remaining account rows in {table}");
    }
    for table in ["sync_state", "sync_library_revision"] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE library_id=$1")).bind(&library_id).fetch_one(pool).await.unwrap();
        assert_eq!(count, 0, "remaining library rows in {table}");
    }
    let queued: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM account_email_outbox WHERE recipient=$1").bind(&email).fetch_one(pool).await.unwrap();
    let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pending_registration WHERE email=$1").bind(&email).fetch_one(pool).await.unwrap();
    assert_eq!((queued, pending), (0, 0));
    assert!(matches!(accounts.resolve_token(&session.token).await, Err(AuthError::InvalidCredentials)));
    assert!(matches!(accounts.resolve_token(&second.token).await, Err(AuthError::InvalidCredentials)));
    assert!(matches!(accounts.refresh_session(&session.refresh_token).await, Err(AuthError::InvalidCredentials)));
    assert!(matches!(accounts.refresh_session(&second.refresh_token).await, Err(AuthError::InvalidCredentials)));
    assert!(matches!(accounts.login(&email, password).await, Err(AuthError::InvalidCredentials)));
    assert_eq!(delete(&app, &email, password, "DELETE").await, StatusCode::UNAUTHORIZED);
    assert_eq!(accounts.resolve_token(&other_session.token).await.unwrap().user_id, other.user_id);

    // The per-email limit applies to attempted password checks.
    let limited = format!("limited-{unique}@example.com");
    for _ in 0..5 {
        assert_eq!(delete(&app, &limited, "wrong-password", "DELETE").await, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(delete(&app, &limited, "wrong-password", "DELETE").await, StatusCode::TOO_MANY_REQUESTS);
    accounts.delete_account(&other_email, password).await.unwrap();
}
