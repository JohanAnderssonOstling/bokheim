use axum::extract::{Json, State};
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};

use crate::auth::{self, SessionRecord};
use crate::routes::extract::{auth_error_to_status, AuthUser};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
pub struct SignupRequest {
    username: String,
    password: String,
    invite: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    token: String,
    user_id: String,
    username: String,
}

#[derive(Debug, Serialize)]
pub struct MeResponse {
    user_id: String,
    username: String,
}

pub async fn post_login(State(state): State<AppState>, Json(req): Json<LoginRequest>) -> Result<Json<AuthResponse>, StatusCode> {
    let (token, SessionRecord { user_id, username }) =
        auth::login(&state.pool, &req.username, &req.password).await.map_err(auth_error_to_status)?;
    Ok(Json(AuthResponse { token, user_id, username }))
}

pub async fn post_logout(State(state): State<AppState>, user: AuthUser, headers: HeaderMap) -> Result<StatusCode, StatusCode> {
    // We have the user from extraction; pull the raw token to invalidate it.
    if let Some(h) = headers.get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(token) = h.strip_prefix("Bearer ") {
            let _ = auth::logout(&state.pool, token.trim()).await;
        }
    }
    let _ = user;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn post_signup(State(state): State<AppState>, Json(req): Json<SignupRequest>) -> Result<Json<AuthResponse>, StatusCode> {
    let (token, SessionRecord { user_id, username }) =
        auth::signup(&state.pool, &req.invite, &req.username, &req.password).await.map_err(auth_error_to_status)?;
    Ok(Json(AuthResponse { token, user_id, username }))
}

pub async fn get_me(user: AuthUser) -> Json<MeResponse> {
    Json(MeResponse { user_id: user.user_id, username: user.username })
}
