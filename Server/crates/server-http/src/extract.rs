use crate::state::AppState;
use axum::async_trait;
use axum::extract::FromRequestParts;
use axum::http::{request::Parts, StatusCode};

pub(crate) struct AuthUser {
    pub(crate) user_id: String,
    pub(crate) username: String,
    pub(crate) bearer_token: String,
}

#[async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let header = parts.headers.get(axum::http::header::AUTHORIZATION).and_then(|value| value.to_str().ok()).ok_or(StatusCode::UNAUTHORIZED)?;
        let token = header.strip_prefix("Bearer ").filter(|token| !token.is_empty()).ok_or(StatusCode::UNAUTHORIZED)?;
        let session = state.auth.resolve_token(token).await.map_err(auth_error)?;
        Ok(Self { user_id: session.user_id, username: session.username, bearer_token: token.to_owned() })
    }
}

pub(crate) fn auth_error(error: server_core::AuthError) -> StatusCode {
    match error {
        server_core::AuthError::InvalidCredentials | server_core::AuthError::InvalidInvite => StatusCode::UNAUTHORIZED,
        server_core::AuthError::UsernameTaken => StatusCode::CONFLICT,
        server_core::AuthError::Forbidden => StatusCode::FORBIDDEN,
        server_core::AuthError::InvalidPassword { .. } | server_core::AuthError::EmptyUsername | server_core::AuthError::InvalidUsername => StatusCode::BAD_REQUEST,
        server_core::AuthError::Internal(reason) => {
            tracing_fallback("authentication", &reason);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

pub(crate) fn sync_error(error: server_core::SyncError) -> StatusCode {
    match error {
        server_core::SyncError::Forbidden => StatusCode::FORBIDDEN,
        server_core::SyncError::LibraryOwnershipConflict => StatusCode::CONFLICT,
        server_core::SyncError::UpgradeRequired => StatusCode::UPGRADE_REQUIRED,
        server_core::SyncError::InvalidLibraryName => StatusCode::BAD_REQUEST,
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
