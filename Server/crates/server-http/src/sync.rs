use crate::extract::{sync_error, AuthUser};
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use sync_common::api::libraries::UpsertLibraryRequest;
use sync_common::api::notifications::NotificationMessage;
use sync_common::SyncExchangeRequest;

pub(super) async fn list_libraries(State(state): State<AppState>, user: AuthUser, headers: HeaderMap) -> Result<Response, axum::http::StatusCode> {
    let libraries = state.sync.list_libraries(&user.user_id).await.map_err(sync_error)?;
    server_wire_http::response(&headers, libraries)
}

pub(super) async fn upsert_library(State(state): State<AppState>, user: AuthUser, headers: HeaderMap, body: Bytes) -> Result<Response, axum::http::StatusCode> {
    let request: UpsertLibraryRequest = server_wire_http::decode_request(&headers, &body, wire::MAX_DECODED_REQUEST_BYTES)?;
    let outcome = state.sync.upsert_library(&user.user_id, &request).await.map_err(sync_error)?.ok_or_else(|| sync_error(server_postgres::SyncError::LibraryOwnershipConflict))?;
    if outcome.changed {
        state.notifications.publish(&user.user_id, Some(user.credential), NotificationMessage::CatalogueChanged);
    }
    server_wire_http::response(&headers, outcome.library)
}

pub(super) async fn exchange_events(State(state): State<AppState>, user: AuthUser, headers: HeaderMap, body: Bytes) -> Result<Response, axum::http::StatusCode> {
    let request: SyncExchangeRequest = server_wire_http::decode_request(&headers, &body, wire::MAX_DECODED_REQUEST_BYTES)?;
    let library_id = request.library_id.clone();
    let response = state.sync.exchange(&user.user_id, &request).await.map_err(sync_error)?;
    if !response.push.accepted.is_empty() {
        state.notifications.publish(&user.user_id, Some(user.credential.clone()), NotificationMessage::LibraryChanged { library_id });
    }
    server_wire_http::response(&headers, response)
}
