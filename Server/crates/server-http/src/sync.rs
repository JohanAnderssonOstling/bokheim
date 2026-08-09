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
    crate::api_wire::response(&headers, libraries)
}

pub(super) async fn upsert_library(State(state): State<AppState>, user: AuthUser, headers: HeaderMap, body: Bytes) -> Result<Response, axum::http::StatusCode> {
    let request: UpsertLibraryRequest = crate::api_wire::decode_request(&headers, &body)?;
    let outcome = state.sync.upsert_library(&user.user_id, &request).await.map_err(sync_error)?.ok_or_else(|| sync_error(server_core::SyncError::LibraryOwnershipConflict))?;
    if outcome.changed {
        state.notifications.publish(&user.user_id, Some(user.credential), NotificationMessage::CatalogueChanged);
    }
    crate::api_wire::response(&headers, outcome.library)
}

pub(super) async fn exchange_events(State(state): State<AppState>, user: AuthUser, headers: HeaderMap, body: Bytes) -> Result<Response, axum::http::StatusCode> {
    let request: SyncExchangeRequest = crate::api_wire::decode_request(&headers, &body)?;
    let library_id = request.library_id.clone();
    let response = state.sync.exchange(&user.user_id, &request).await.map_err(sync_error)?;
    if !response.push.accepted.is_empty() {
        state.notifications.publish(&user.user_id, Some(user.credential.clone()), NotificationMessage::LibraryChanged { library_id });
    }
    crate::api_wire::response(&headers, response)
}
