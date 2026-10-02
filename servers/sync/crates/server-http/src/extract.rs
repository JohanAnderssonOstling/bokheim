use crate::state::AppState;
use axum::async_trait;
use axum::extract::FromRequestParts;
use axum::http::{request::Parts, StatusCode};
use std::sync::Arc;

pub(crate) struct AuthUser {
    pub(crate) user_id: String,
    pub(crate) credential: Arc<str>,
}

#[async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let bearer = parts.headers.get(axum::http::header::AUTHORIZATION).and_then(|value| value.to_str().ok()).and_then(|header| header.strip_prefix("Bearer ")).filter(|token| !token.is_empty());
        let bearer = bearer.or(crate::notifications::websocket_bearer(parts.uri.path(), &parts.headers)?);
        let token = bearer.ok_or(StatusCode::UNAUTHORIZED)?;
        let session = state.account.resolve_token(token).await.map_err(server_account::auth_error)?;
        Ok(Self { user_id: session.user_id, credential: Arc::from(token) })
    }
}

pub(crate) fn sync_error(error: server_postgres::SyncError) -> StatusCode {
    match error {
        server_postgres::SyncError::Forbidden => StatusCode::FORBIDDEN,
        server_postgres::SyncError::LibraryOwnershipConflict => StatusCode::CONFLICT,
        server_postgres::SyncError::CursorAhead => StatusCode::GONE,
        server_postgres::SyncError::UpgradeRequired => StatusCode::UPGRADE_REQUIRED,
        server_postgres::SyncError::InvalidLibraryName => StatusCode::BAD_REQUEST,
        server_postgres::SyncError::AmbiguousMutationIdentity => StatusCode::BAD_REQUEST,
        server_postgres::SyncError::TooManyMutations => StatusCode::PAYLOAD_TOO_LARGE,
        server_postgres::SyncError::Internal(reason) => {
            tracing_fallback("synchronization", &reason);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

pub(crate) fn asset_error(error: server_asset_store::AssetError) -> StatusCode {
    match error {
        server_asset_store::AssetError::Forbidden => StatusCode::FORBIDDEN,
        server_asset_store::AssetError::InvalidInput(_) | server_asset_store::AssetError::HashMismatch | server_asset_store::AssetError::LengthMismatch { .. } => StatusCode::BAD_REQUEST,
        server_asset_store::AssetError::LengthRequired => StatusCode::LENGTH_REQUIRED,
        server_asset_store::AssetError::UploadInProgress => StatusCode::CONFLICT,
        server_asset_store::AssetError::Busy => StatusCode::TOO_MANY_REQUESTS,
        server_asset_store::AssetError::QuotaExceeded { .. } => StatusCode::INSUFFICIENT_STORAGE,
        server_asset_store::AssetError::TooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        server_asset_store::AssetError::RangeNotSatisfiable { .. } => StatusCode::RANGE_NOT_SATISFIABLE,
        server_asset_store::AssetError::Storage(reason) => {
            tracing_fallback("asset storage", &reason);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

fn tracing_fallback(component: &str, reason: &str) {
    eprintln!("{component} error: {reason}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_regression_and_library_ownership_have_distinct_statuses() {
        assert_eq!(sync_error(server_postgres::SyncError::CursorAhead), StatusCode::GONE);
        assert_eq!(sync_error(server_postgres::SyncError::LibraryOwnershipConflict), StatusCode::CONFLICT);
    }
}
