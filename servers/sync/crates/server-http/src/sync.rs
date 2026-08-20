use crate::extract::{sync_error, AuthUser};
use crate::state::AppState;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::response::Response;
use sync_common::api::libraries::{DeleteLibraryResponse, DeletedLibrariesResponse, UpsertLibraryRequest};
use sync_common::api::notifications::NotificationMessage;
use sync_common::LibraryId;
use sync_common::{StateInventoryRequest, SyncExchangeRequest};

pub(super) async fn list_libraries(State(state): State<AppState>, user: AuthUser, headers: HeaderMap) -> Result<Response, axum::http::StatusCode> {
    let libraries = state.sync.list_libraries(&user.user_id).await.map_err(sync_error)?;
    server_wire_http::response(&headers, libraries)
}

pub(super) async fn list_deleted_libraries(State(state): State<AppState>, user: AuthUser, headers: HeaderMap) -> Result<Response, axum::http::StatusCode> {
    let library_ids = state.sync.list_deleted_libraries(&user.user_id).await.map_err(sync_error)?;
    server_wire_http::response(&headers, DeletedLibrariesResponse { library_ids })
}

pub(super) async fn upsert_library(State(state): State<AppState>, user: AuthUser, headers: HeaderMap, body: Bytes) -> Result<Response, axum::http::StatusCode> {
    let request: UpsertLibraryRequest = server_wire_http::decode_request(&headers, &body, wire::MAX_DECODED_REQUEST_BYTES)?;
    let outcome = state.sync.upsert_library(&user.user_id, &request).await.map_err(sync_error)?.ok_or_else(|| sync_error(server_postgres::SyncError::LibraryOwnershipConflict))?;
    if outcome.changed {
        state.notifications.publish(&user.user_id, Some(user.credential), NotificationMessage::LibrariesChanged);
    }
    server_wire_http::response(&headers, outcome.library)
}

pub(super) async fn delete_library(State(state): State<AppState>, user: AuthUser, headers: HeaderMap, Path(library_id): Path<String>) -> Result<Response, axum::http::StatusCode> {
    let library_id = LibraryId::parse_str(&library_id).map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;
    if state.sync.delete_library(&user.user_id, &library_id).await.map_err(sync_error)? {
        state.notifications.publish(&user.user_id, Some(user.credential), NotificationMessage::LibrariesChanged);
    }
    server_wire_http::response(&headers, DeleteLibraryResponse { library_id })
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

pub(super) async fn compare_state_inventory(State(state): State<AppState>, user: AuthUser, headers: HeaderMap, body: Bytes) -> Result<Response, axum::http::StatusCode> {
    let request: StateInventoryRequest = server_wire_http::decode_request(&headers, &body, wire::MAX_DECODED_REQUEST_BYTES)?;
    let response = state.sync.compare_state_inventory(&user.user_id, &request).await.map_err(sync_error)?;
    server_wire_http::response(&headers, response)
}
