use axum::extract::{Json, State};
use axum::http::{HeaderMap, StatusCode};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path as StdPath;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::auth;
use crate::db;
use crate::routes::extract::auth_error_to_status;
use crate::state::AppState;

#[derive(Debug, Deserialize, Default)]
pub struct CreateInviteRequest {
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    expires_in_days: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct CreateInviteResponse {
    invite: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

pub async fn post_admin_invite(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Option<Json<CreateInviteRequest>>,
) -> Result<Json<CreateInviteResponse>, StatusCode> {
    let basic = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    auth::verify_admin_basic(&state.pool, basic).await.map_err(auth_error_to_status)?;

    let req = body.map(|Json(b)| b).unwrap_or_default();
    let (invite, expires_at) = auth::create_invite(&state.pool, req.note.as_deref(), req.expires_in_days)
        .await
        .map_err(auth_error_to_status)?;
    Ok(Json(CreateInviteResponse { invite, expires_at }))
}

#[derive(Debug, Serialize)]
pub struct GcReport {
    blobs_scanned: usize,
    blobs_deleted: usize,
    thumbnails_scanned: usize,
    thumbnails_deleted: usize,
    bytes_reclaimed: u64,
}

const GC_SAFETY_WINDOW: Duration = Duration::from_secs(3600);

/// Delete blob and thumbnail files not referenced by any live `book_dir_added`
/// event. Files modified within the safety window are skipped to avoid racing
/// in-flight uploads whose creating event hasn't been pushed yet.
pub async fn post_admin_gc(State(state): State<AppState>) -> Result<Json<GcReport>, StatusCode> {
    let live = db::live_blob_hashes(&state.pool).await.map_err(|err| {
        eprintln!("gc: live_blob_hashes failed: {err:?}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let cutoff = SystemTime::now().checked_sub(GC_SAFETY_WINDOW).unwrap_or(UNIX_EPOCH);

    let (blobs_scanned, blobs_deleted, blob_bytes) = sweep_dir(&state.books_dir, &live, cutoff, true).await;
    let (thumbnails_scanned, thumbnails_deleted, thumb_bytes) = sweep_dir(&state.thumbnails_dir, &live, cutoff, false).await;

    Ok(Json(GcReport {
        blobs_scanned,
        blobs_deleted,
        thumbnails_scanned,
        thumbnails_deleted,
        bytes_reclaimed: blob_bytes + thumb_bytes,
    }))
}

/// Walks `root`, deleting any file whose stem isn't in `live` and whose mtime
/// is older than `cutoff`. Returns (scanned, deleted, bytes_reclaimed).
async fn sweep_dir(root: &StdPath, live: &HashSet<String>, cutoff: SystemTime, sharded: bool) -> (usize, usize, u64) {
    let mut scanned = 0usize;
    let mut deleted = 0usize;
    let mut bytes = 0u64;

    let mut top = match tokio::fs::read_dir(root).await {
        Ok(rd) => rd,
        Err(_) => return (0, 0, 0),
    };
    while let Ok(Some(entry)) = top.next_entry().await {
        let path = entry.path();
        if sharded {
            if !path.is_dir() {
                continue;
            }
            let mut inner = match tokio::fs::read_dir(&path).await {
                Ok(rd) => rd,
                Err(_) => continue,
            };
            while let Ok(Some(file)) = inner.next_entry().await {
                let (s, d, b) = consider_file(&file.path(), live, cutoff).await;
                scanned += s;
                deleted += d;
                bytes += b;
            }
        } else {
            let (s, d, b) = consider_file(&path, live, cutoff).await;
            scanned += s;
            deleted += d;
            bytes += b;
        }
    }
    (scanned, deleted, bytes)
}

async fn consider_file(path: &StdPath, live: &HashSet<String>, cutoff: SystemTime) -> (usize, usize, u64) {
    let meta = match tokio::fs::metadata(path).await {
        Ok(m) => m,
        Err(_) => return (0, 0, 0),
    };
    if !meta.is_file() {
        return (0, 0, 0);
    }
    // Skip in-flight uploads.
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name.ends_with(".tmp") {
            return (0, 0, 0);
        }
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if live.contains(stem) {
        return (1, 0, 0);
    }
    if meta.modified().map(|m| m > cutoff).unwrap_or(true) {
        // Younger than the safety window — skip this round.
        return (1, 0, 0);
    }
    let len = meta.len();
    if tokio::fs::remove_file(path).await.is_ok() {
        return (1, 1, len);
    }
    (1, 0, 0)
}
