use crate::extract::{asset_error, AuthUser};
use crate::state::AppState;
use account_contract::{LibraryStorageUsage, LibraryStorageUsageResponse, StorageUsageResponse};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;

pub(crate) async fn get_storage_usage(State(state): State<AppState>, user: AuthUser, headers: HeaderMap) -> Result<Response, StatusCode> {
    let usage = state.assets.storage_usage(&user.user_id).await.map_err(asset_error)?;
    server_wire_http::response(&headers, StorageUsageResponse { used_bytes: usage.used_bytes, reserved_bytes: usage.reserved_bytes, quota_bytes: usage.quota_bytes })
}

/// The per-library split of the same charged bytes `get_storage_usage` totals.
pub(crate) async fn get_library_storage_usage(State(state): State<AppState>, user: AuthUser, headers: HeaderMap) -> Result<Response, StatusCode> {
    let usage = state.assets.library_storage_usage(&user.user_id).await.map_err(asset_error)?;
    let libraries = usage.into_iter().map(|entry| LibraryStorageUsage { library_id: entry.library_id.to_string(), used_bytes: entry.used_bytes }).collect();
    server_wire_http::response(&headers, LibraryStorageUsageResponse { libraries })
}
