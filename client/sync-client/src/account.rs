use crate::api::{receive, send};
use crate::session::{validate_email, validate_identity, AuthError, Session};
use account_contract::{MeResponse, StorageUsageResponse};
use sync_common::api::libraries::{DeleteLibraryResponse, LibrarySummary, UpsertLibraryRequest};

pub async fn current_account(session: &Session) -> Result<MeResponse, AuthError> {
    let account: MeResponse = receive(reqwest::Client::builder().build()?.get(session.server_url().endpoint("api/auth/me")).bearer_auth(session.token())).await?;
    validate_identity(&account.user_id, "user id")?;
    validate_email(&account.email)?;
    if account.user_id != session.user_id() || account.email != session.email() {
        return Err(AuthError::InvalidData("session identity does not match the identity authority".to_owned()));
    }
    Ok(account)
}

pub async fn account_storage_usage(session: &Session) -> Result<StorageUsageResponse, AuthError> {
    receive(reqwest::Client::builder().build()?.get(session.server_url().endpoint("api/account/storage")).bearer_auth(session.token())).await
}

/// The per-library split of the charged bytes.
///
/// A route older servers do not have; the caller decides what an absent
/// breakdown means rather than this layer inventing zeroes.
pub async fn account_library_storage_usage(session: &Session) -> Result<account_contract::LibraryStorageUsageResponse, AuthError> {
    receive(reqwest::Client::builder().build()?.get(session.server_url().endpoint("api/account/storage/libraries")).bearer_auth(session.token())).await
}

pub async fn list_account_sessions(session: &Session) -> Result<Vec<account_contract::SessionSummary>, AuthError> {
    receive(reqwest::Client::builder().build()?.get(session.server_url().endpoint("api/auth/sessions")).bearer_auth(session.token())).await
}

pub async fn revoke_account_session(session: &Session, session_id: &str) -> Result<account_contract::SessionRevocationResponse, AuthError> {
    validate_identity(session_id, "session id")?;
    let endpoint = format!("api/auth/sessions/{}", urlencoding::encode(session_id));
    receive(reqwest::Client::builder().build()?.delete(session.server_url().endpoint(&endpoint)).bearer_auth(session.token())).await
}

pub async fn revoke_other_account_sessions(session: &Session) -> Result<account_contract::OtherSessionsRevocationResponse, AuthError> {
    receive(reqwest::Client::builder().build()?.delete(session.server_url().endpoint("api/auth/sessions")).bearer_auth(session.token())).await
}

pub async fn list_libraries(session: &Session) -> Result<Vec<LibrarySummary>, AuthError> {
    receive(reqwest::Client::builder().build()?.get(session.server_url().endpoint("api/libraries")).bearer_auth(session.token())).await
}

pub async fn list_deleted_libraries(session: &Session) -> Result<sync_common::api::libraries::DeletedLibrariesResponse, AuthError> {
    receive(reqwest::Client::builder().build()?.get(session.server_url().endpoint("api/libraries/deleted")).bearer_auth(session.token())).await
}

pub async fn upsert_library(session: &Session, library_id: sync_common::LibraryId, library_name: String) -> Result<LibrarySummary, AuthError> {
    let request = UpsertLibraryRequest { library_id, library_name };
    send(reqwest::Client::builder().build()?.post(session.server_url().endpoint("api/libraries")).bearer_auth(session.token()), &request).await
}

pub async fn delete_library(session: &Session, library_id: &sync_common::LibraryId) -> Result<DeleteLibraryResponse, AuthError> {
    receive(reqwest::Client::builder().build()?.delete(session.server_url().endpoint(&format!("api/libraries/{library_id}"))).bearer_auth(session.token())).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServerUrl;
    use axum::http::{header, HeaderMap, HeaderValue};
    use axum::response::IntoResponse;
    use axum::routing::get;
    use axum::Router;
    use sync_common::transport as wire;

    const TEST_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[tokio::test]
    async fn storage_usage_uses_the_authenticated_binary_endpoint() {
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
}
