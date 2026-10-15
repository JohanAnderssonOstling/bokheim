use crate::http::{authenticated_request, decode_response, send, with_body};
use crate::session::{validate_email, validate_identity, AuthError, Session};
use account_contract::{AccountSnapshotResponse, MeResponse, OtherSessionsRevocationResponse, SessionRevocationResponse, SessionSummary};
use reqwest::Method;
use sync_common::api::libraries::{CloudStorageChange, DeleteLibraryResponse, LibraryCloudStorage, LibraryNameRequest, LibrarySummary};
use sync_common::api::opds_feeds::{OpdsFeed, OpdsFeedId};
use sync_common::LibraryId;

pub async fn current_account(session: &Session) -> Result<MeResponse, AuthError> {
    let account: MeResponse = decode_response(authenticated_request(session, Method::GET, "api/auth/me")?).await?;
    validate_identity(&account.user_id, "user id")?;
    validate_email(&account.email)?;
    if account.user_id != session.user_id() || account.email != session.email() {
        return Err(AuthError::InvalidData("session identity does not match the identity authority".to_owned()));
    }
    Ok(account)
}

pub async fn account_snapshot(session: &Session) -> Result<AccountSnapshotResponse, AuthError> {
    decode_response(authenticated_request(session, Method::GET, "api/account/snapshot")?).await
}

pub async fn list_account_sessions(session: &Session) -> Result<Vec<SessionSummary>, AuthError> {
    decode_response(authenticated_request(session, Method::GET, "api/auth/sessions")?).await
}

pub async fn revoke_account_session(session: &Session, session_id: &str) -> Result<SessionRevocationResponse, AuthError> {
    validate_identity(session_id, "session id")?;
    let endpoint = format!("api/auth/sessions/{}", urlencoding::encode(session_id));
    decode_response(authenticated_request(session, Method::DELETE, &endpoint)?).await
}

pub async fn revoke_other_account_sessions(session: &Session) -> Result<OtherSessionsRevocationResponse, AuthError> {
    decode_response(authenticated_request(session, Method::DELETE, "api/auth/sessions")?).await
}

pub async fn create_library(session: &Session, library_id: LibraryId, library_name: String) -> Result<LibrarySummary, AuthError> {
    let request = LibraryNameRequest { library_id, library_name };
    decode_response(with_body(authenticated_request(session, Method::POST, "api/libraries")?, &request)?).await
}

pub async fn rename_library(session: &Session, library_id: LibraryId, library_name: String) -> Result<LibrarySummary, AuthError> {
    let request = LibraryNameRequest { library_id, library_name };
    decode_response(with_body(authenticated_request(session, Method::PUT, "api/libraries")?, &request)?).await
}

pub async fn delete_library(session: &Session, library_id: &LibraryId) -> Result<DeleteLibraryResponse, AuthError> {
    decode_response(authenticated_request(session, Method::DELETE, &format!("api/libraries/{library_id}"))?).await
}

pub async fn opds_feeds(session: &Session) -> Result<Vec<OpdsFeed>, AuthError> {
    decode_response(authenticated_request(session, Method::GET, "api/opds-feeds")?).await
}

pub async fn put_opds_feed(session: &Session, feed: &OpdsFeed) -> Result<OpdsFeed, AuthError> {
    decode_response(with_body(authenticated_request(session, Method::PUT, "api/opds-feeds")?, feed)?).await
}

pub async fn delete_opds_feed(session: &Session, id: OpdsFeedId) -> Result<(), AuthError> {
    send(authenticated_request(session, Method::DELETE, &format!("api/opds-feeds/{id}"))?).await?;
    Ok(())
}

pub async fn cloud_storage(session: &Session) -> Result<Vec<LibraryCloudStorage>, AuthError> {
    decode_response(authenticated_request(session, Method::GET, "api/cloud-storage")?).await
}

pub async fn set_cloud_storage(session: &Session, id: LibraryId, change: CloudStorageChange) -> Result<LibraryCloudStorage, AuthError> {
    decode_response(with_body(authenticated_request(session, Method::PUT, &format!("api/libraries/{id}/cloud-storage"))?, &change)?).await
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::Url;
    use axum::body::Bytes;
    use axum::extract::Path;
    use axum::http::{header, HeaderMap, StatusCode};
    use axum::response::IntoResponse;
    use axum::routing::{delete, post};
    use axum::Router;
    use sync_common::transport as wire;

    const TEST_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const LIBRARY_ID: &str = "00000000-0000-4000-8000-000000000001";

    fn check_auth(headers: &HeaderMap) {
        assert_eq!(headers.get(header::AUTHORIZATION).unwrap(), format!("Bearer {TEST_TOKEN}").as_str());
        assert_eq!(headers.get(header::ACCEPT).unwrap(), wire::MEDIA_TYPE);
    }

    async fn write_library(method: Method, headers: HeaderMap, body: Bytes) -> Vec<u8> {
        check_auth(&headers);
        assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), wire::MEDIA_TYPE);
        let request: LibraryNameRequest = wire::decode(&body, wire::MAX_DECODED_REQUEST_BYTES).unwrap();
        assert_eq!(request.library_id.to_string(), LIBRARY_ID);
        assert_eq!(request.library_name, if method == Method::POST { "Classics" } else { "Renamed" });
        wire::encode(&LibrarySummary { library_id: request.library_id, library_name: request.library_name }).unwrap()
    }

    #[tokio::test]
    async fn account_mutations_preserve_methods_headers_bodies_and_status_errors() {
        let app = Router::new()
            .route("/api/libraries", post(write_library).put(write_library))
            .route(
                "/api/libraries/:id",
                delete(|Path(id): Path<LibraryId>, headers: HeaderMap| async move {
                    check_auth(&headers);
                    assert!(headers.get(header::CONTENT_TYPE).is_none());
                    if id.is_nil() {
                        return StatusCode::UNAUTHORIZED.into_response();
                    }
                    assert_eq!(id.to_string(), LIBRARY_ID);
                    wire::encode(&DeleteLibraryResponse { library_id: id }).unwrap().into_response()
                }),
            )
            .route(
                "/api/opds-feeds/:id",
                delete(|Path(id): Path<OpdsFeedId>, headers: HeaderMap| async move {
                    check_auth(&headers);
                    assert_eq!(id.to_string(), LIBRARY_ID);
                    StatusCode::NO_CONTENT
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let session = Session::try_new(origin, "user-1".into(), "reader@example.com".into(), TEST_TOKEN.into()).unwrap();
        let id = LIBRARY_ID.parse().unwrap();

        assert_eq!(create_library(&session, id, "Classics".into()).await.unwrap(), LibrarySummary { library_id: id, library_name: "Classics".into() });
        assert_eq!(rename_library(&session, id, "Renamed".into()).await.unwrap(), LibrarySummary { library_id: id, library_name: "Renamed".into() });
        assert_eq!(delete_library(&session, &id).await.unwrap().library_id, id);
        delete_opds_feed(&session, id).await.unwrap();
        assert!(delete_library(&session, &LibraryId::nil()).await.unwrap_err().is_unauthorized());
        server.abort();
    }
}
