use crate::extract::{asset_error, sync_error, AuthUser};
use crate::state::AppState;
use account_contract::{AccountSnapshotResponse, BookStorageUsage, LibraryStorageUsage, StorageUsageResponse, MAX_ACCOUNT_STORAGE_BOOKS};
use axum::extract::State;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;

pub(crate) async fn get_entitlements(State(state): State<AppState>, user: AuthUser, headers: HeaderMap) -> Result<Response, StatusCode> {
    let entitlements = state.billing.entitlements(&user.user_id).await.map_err(|error| {
        if matches!(error, server_postgres::BillingError::UnknownAccount) {
            StatusCode::UNAUTHORIZED
        } else {
            tracing::error!(%error, "account entitlements failed");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    })?;
    let mut response = server_wire_http::response(&headers, entitlements)?;
    response.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}

pub(crate) async fn get_snapshot(State(state): State<AppState>, user: AuthUser, headers: HeaderMap) -> Result<Response, StatusCode> {
    let user_id = &user.user_id;
    let (libraries, deleted_library_ids, storage, library_storage) = tokio::try_join!(
        async { state.sync.list_libraries(user_id).await.map_err(sync_error) },
        async { state.sync.list_deleted_libraries(user_id).await.map_err(sync_error) },
        async { state.assets.storage_usage(user_id).await.map_err(asset_error) },
        async { state.assets.library_storage_usage(user_id).await.map_err(asset_error) },
    )?;
    let storage = StorageUsageResponse { used_bytes: storage.used_bytes, reserved_bytes: storage.reserved_bytes, quota_bytes: storage.quota_bytes };
    let library_storage = library_storage.into_iter().map(|entry| LibraryStorageUsage { library_id: entry.library_id.to_string(), used_bytes: entry.used_bytes }).collect();
    server_wire_http::response(&headers, AccountSnapshotResponse { libraries, deleted_library_ids, storage, library_storage })
}

pub(crate) async fn get_book_storage(State(state): State<AppState>, user: AuthUser, headers: HeaderMap) -> Result<Response, StatusCode> {
    let books = state.assets.book_storage_usage(&user.user_id, MAX_ACCOUNT_STORAGE_BOOKS).await.map_err(asset_error)?;
    let books: Vec<_> = books.into_iter().map(|book| BookStorageUsage { book_id: book.book_id, used_bytes: book.used_bytes }).collect();
    server_wire_http::response(&headers, books)
}
