use axum::async_trait;
use axum::extract::FromRequestParts;
use axum::http::{request::Parts, StatusCode};

use crate::auth::{self, AuthError};
use crate::state::AppState;

/// Authenticated user resolved from the `Authorization: Bearer` header. Any
/// handler that takes this as an argument is gated behind a valid token.
#[derive(Debug)]
pub struct AuthUser {
    pub user_id: String,
    pub username: String,
}

#[async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(StatusCode::UNAUTHORIZED)?;
        let token = header.strip_prefix("Bearer ").ok_or(StatusCode::UNAUTHORIZED)?.trim();
        let session = auth::resolve_token(&state.pool, token).await.map_err(|_| StatusCode::UNAUTHORIZED)?;
        Ok(AuthUser { user_id: session.user_id, username: session.username })
    }
}

pub fn auth_error_to_status(err: AuthError) -> StatusCode {
    match err {
        AuthError::InvalidCredentials | AuthError::InvalidInvite => StatusCode::UNAUTHORIZED,
        AuthError::UsernameTaken => StatusCode::CONFLICT,
        AuthError::Forbidden => StatusCode::FORBIDDEN,
        AuthError::WeakPassword(_) | AuthError::EmptyUsername => StatusCode::BAD_REQUEST,
        AuthError::Db(e) => {
            eprintln!("auth db error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
        AuthError::Hash(e) => {
            eprintln!("auth hash error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
