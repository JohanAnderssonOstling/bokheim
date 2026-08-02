use crate::extract::auth_error;
use crate::state::AppState;
use axum::extract::{Json, State};
use axum::http::{HeaderMap, StatusCode};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Deserialize, Default)]
pub(super) struct CreateInviteRequest {
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    expires_in_days: Option<i64>,
}

#[derive(Debug, Serialize)]
pub(super) struct CreateInviteResponse {
    invite: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize)]
pub(super) struct GcReport {
    blobs_scanned: usize,
    blobs_deleted: usize,
    thumbnails_scanned: usize,
    thumbnails_deleted: usize,
    bytes_reclaimed: u64,
}

const GC_SAFETY_WINDOW: Duration = Duration::from_secs(3600);

fn basic_credentials(headers: &HeaderMap) -> Option<(String, String)> {
    let encoded = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Basic ")?;
    let decoded = STANDARD.decode(encoded.trim()).ok()?;
    let decoded = String::from_utf8(decoded).ok()?;
    let (username, password) = decoded.split_once(':')?;
    Some((username.to_string(), password.to_string()))
}

pub(super) async fn post_admin_invite(State(state): State<AppState>, headers: HeaderMap, body: Option<Json<CreateInviteRequest>>) -> Result<Json<CreateInviteResponse>, StatusCode> {
    let (username, password) = basic_credentials(&headers).ok_or(StatusCode::UNAUTHORIZED)?;
    state.auth.verify_admin(&username, &password).await.map_err(auth_error)?;
    let request = body.map(|Json(body)| body).unwrap_or_default();
    let (invite, expires_at) = state.auth.create_invite(request.note.as_deref(), request.expires_in_days).await.map_err(auth_error)?;
    Ok(Json(CreateInviteResponse { invite, expires_at }))
}

pub(super) async fn post_admin_gc(State(state): State<AppState>) -> Result<Json<GcReport>, StatusCode> {
    let cutoff = SystemTime::now().checked_sub(GC_SAFETY_WINDOW).unwrap_or(UNIX_EPOCH);
    let report = state.garbage_collector.collect(cutoff).await.map_err(crate::extract::sync_error)?;
    Ok(Json(GcReport { blobs_scanned: report.blobs_scanned, blobs_deleted: report.blobs_deleted, thumbnails_scanned: report.thumbnails_scanned, thumbnails_deleted: report.thumbnails_deleted, bytes_reclaimed: report.bytes_reclaimed }))
}
