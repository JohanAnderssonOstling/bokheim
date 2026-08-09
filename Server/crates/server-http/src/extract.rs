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
        let header = parts.headers.get(axum::http::header::AUTHORIZATION).and_then(|value| value.to_str().ok()).ok_or(StatusCode::UNAUTHORIZED)?;
        let token = header.strip_prefix("Bearer ").filter(|token| !token.is_empty()).ok_or(StatusCode::UNAUTHORIZED)?;
        let session = state.account.resolve_token(token).await.map_err(server_account::auth_error)?;
        Ok(Self { user_id: session.user_id, credential: Arc::from(token) })
    }
}

pub(crate) fn sync_error(error: server_core::SyncError) -> StatusCode {
    match error {
        server_core::SyncError::Forbidden => StatusCode::FORBIDDEN,
        server_core::SyncError::LibraryOwnershipConflict => StatusCode::CONFLICT,
        server_core::SyncError::CursorAhead => StatusCode::CONFLICT,
        server_core::SyncError::UpgradeRequired => StatusCode::UPGRADE_REQUIRED,
        server_core::SyncError::InvalidLibraryName => StatusCode::BAD_REQUEST,
        server_core::SyncError::AmbiguousMutationIdentity => StatusCode::BAD_REQUEST,
        server_core::SyncError::TooManyMutations => StatusCode::PAYLOAD_TOO_LARGE,
        server_core::SyncError::Internal(reason) => {
            tracing_fallback("synchronization", &reason);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

pub(crate) fn asset_error(error: server_core::AssetError) -> StatusCode {
    match error {
        server_core::AssetError::Forbidden => StatusCode::FORBIDDEN,
        server_core::AssetError::InvalidInput(_) | server_core::AssetError::HashMismatch | server_core::AssetError::LengthMismatch { .. } => StatusCode::BAD_REQUEST,
        server_core::AssetError::LengthRequired => StatusCode::LENGTH_REQUIRED,
        server_core::AssetError::UploadInProgress => StatusCode::CONFLICT,
        server_core::AssetError::Busy => StatusCode::TOO_MANY_REQUESTS,
        server_core::AssetError::QuotaExceeded { .. } => StatusCode::INSUFFICIENT_STORAGE,
        server_core::AssetError::TooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
        server_core::AssetError::RangeNotSatisfiable { .. } => StatusCode::RANGE_NOT_SATISFIABLE,
        server_core::AssetError::Storage(reason) => {
            tracing_fallback("asset storage", &reason);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

fn tracing_fallback(component: &str, reason: &str) {
    eprintln!("{component} error: {reason}");
}
