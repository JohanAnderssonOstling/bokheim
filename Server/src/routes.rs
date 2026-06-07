use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Json, Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    routing::{get, head, post},
    Router,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::collections::HashMap;
use std::path::{Path as StdPath, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex as AsyncMutex;
use tokio_util::io::ReaderStream;

use crate::auth::{self, AuthError, SessionRecord};
use crate::db;
use axum::async_trait;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use sync_common::{PullChangesResponse, PullQuery, PushChangesRequest, PushChangesResponse};

type LibraryLock = Arc<AsyncMutex<()>>;

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub books_dir: PathBuf,
    pub thumbnails_dir: PathBuf,
    pub library_locks: Arc<StdMutex<HashMap<String, LibraryLock>>>,
}

impl AppState {
    fn library_lock(&self, library_id: &str) -> LibraryLock {
        let mut map = self.library_locks.lock().unwrap();
        map.entry(library_id.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }
}

const MAX_UPLOAD_BYTES: usize = 1 * 1024 * 1024 * 1024; // 1 GiB

#[derive(Debug, Deserialize)]
struct ExistsRequest {
    hashes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ThumbnailExistsRequest {
    content_hashes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ExistsResponse {
    present: Vec<String>,
}

fn blob_path(base: &PathBuf, hash: &str) -> PathBuf {
    let pfx = hash.chars().take(2).collect::<String>();
    base.join(pfx).join(hash)
}

fn thumbnail_path(base: &PathBuf, content_hash: &str) -> PathBuf {
    base.join(format!("{content_hash}.png"))
}

pub fn router(pool: PgPool) -> Router {
    let books_dir = PathBuf::from("./books");
    let thumbnails_dir = PathBuf::from("./thumbnails");
    let _ = std::fs::create_dir_all(&books_dir);
    let _ = std::fs::create_dir_all(&thumbnails_dir);
    let state = AppState { pool, books_dir, thumbnails_dir, library_locks: Arc::new(StdMutex::new(HashMap::new())) };
    Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/signup", post(post_signup))
        .route("/api/auth/login", post(post_login))
        .route("/api/auth/logout", post(post_logout))
        .route("/api/auth/me", get(get_me))
        .route("/api/sync/push", post(push_events))
        .route("/api/sync/pull", get(pull_events))
        .route("/api/blobs/:hash", head(head_blob).get(get_blob).put(put_blob))
        .route("/api/blobs/exists", post(post_blobs_exists))
        .route("/api/thumbnails/:content_hash", head(head_thumbnail_blob).get(get_thumbnail_blob).put(put_thumbnail_blob))
        .route("/api/thumbnails/exists", post(post_thumbnails_exists))
        .route("/api/admin/invites", post(post_admin_invite))
        .route("/api/admin/gc", post(post_admin_gc))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES))
        .with_state(state)
}

async fn health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}

async fn push_events(State(state): State<AppState>, user: AuthUser, Json(payload): Json<PushChangesRequest>) -> Result<Json<PushChangesResponse>, StatusCode> {
    if payload.user_id != user.user_id {
        return Err(StatusCode::FORBIDDEN);
    }
    if payload.changes.is_empty() {
        return Ok(Json(PushChangesResponse { handled_local_seqs: Vec::new(), rejected: Vec::new() }));
    }

    let ok = db::ensure_library(&state.pool, &user.user_id, &payload.library_id).await.map_err(|err| {
        eprintln!("ensure_library failed for {} / {}: {:?}", user.user_id, payload.library_id, err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if !ok {
        return Err(StatusCode::CONFLICT);
    }

    let lock = state.library_lock(&payload.library_id);
    let _write_guard = lock.lock().await;

    let mut tx = state.pool.begin().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let result = db::insert_changes(&mut tx, &user.user_id, &payload.library_id, &payload.client_id, &payload.changes).await.map_err(|err| {
        eprintln!("insert_changes failed for {} / {}: {:?}", user.user_id, payload.library_id, err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    tx.commit().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(PushChangesResponse {
        handled_local_seqs: result.handled_local_seqs,
        rejected: result.rejected,
    }))
}

async fn pull_events(State(state): State<AppState>, user: AuthUser, Query(query): Query<PullQuery>) -> Result<Json<PullChangesResponse>, StatusCode> {
    if query.user_id != user.user_id {
        return Err(StatusCode::FORBIDDEN);
    }
    let ok = db::ensure_library(&state.pool, &user.user_id, &query.library_id).await.map_err(|err| {
        eprintln!("ensure_library (pull) failed for {} / {}: {:?}", user.user_id, query.library_id, err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    if !ok {
        return Err(StatusCode::CONFLICT);
    }

    let changes = db::fetch_changes(&state.pool, &query.library_id, &query.client_id, query.since_server_seq).await.map_err(|err| {
        eprintln!("fetch_changes failed for {} / {}: {:?}", user.user_id, query.library_id, err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;

    Ok(Json(PullChangesResponse { changes }))
}

async fn head_blob(State(state): State<AppState>, _user: AuthUser, Path(hash): Path<String>) -> StatusCode {
    let path = blob_path(&state.books_dir, &hash);
    if path.exists() {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn get_blob(State(state): State<AppState>, _user: AuthUser, Path(hash): Path<String>) -> Result<Response, StatusCode> {
    stream_file(&blob_path(&state.books_dir, &hash)).await
}

async fn put_blob(State(state): State<AppState>, _user: AuthUser, Path(hash): Path<String>, body: Body) -> Result<StatusCode, StatusCode> {
    let final_path = blob_path(&state.books_dir, &hash);
    stream_in_verified(&final_path, body, Some(&hash)).await
}

async fn post_blobs_exists(State(state): State<AppState>, _user: AuthUser, Json(req): Json<ExistsRequest>) -> Json<ExistsResponse> {
    let present = req.hashes.into_iter().filter(|hash| blob_path(&state.books_dir, hash).exists()).collect();
    Json(ExistsResponse { present })
}

async fn head_thumbnail_blob(State(state): State<AppState>, _user: AuthUser, Path(content_hash): Path<String>) -> StatusCode {
    let path = thumbnail_path(&state.thumbnails_dir, &content_hash);
    if path.exists() {
        StatusCode::OK
    } else {
        StatusCode::NOT_FOUND
    }
}

async fn get_thumbnail_blob(State(state): State<AppState>, _user: AuthUser, Path(content_hash): Path<String>) -> Result<Response, StatusCode> {
    stream_file(&thumbnail_path(&state.thumbnails_dir, &content_hash)).await
}

async fn put_thumbnail_blob(State(state): State<AppState>, _user: AuthUser, Path(content_hash): Path<String>, body: Body) -> Result<StatusCode, StatusCode> {
    // Thumbnails aren't content-hashed by their own bytes (the URL hash is the
    // book's hash), so we don't verify — just stream and atomically swap.
    stream_in_verified(&thumbnail_path(&state.thumbnails_dir, &content_hash), body, None).await
}

async fn post_thumbnails_exists(State(state): State<AppState>, _user: AuthUser, Json(req): Json<ThumbnailExistsRequest>) -> Json<ExistsResponse> {
    let present = req.content_hashes.into_iter().filter(|hash| thumbnail_path(&state.thumbnails_dir, hash).exists()).collect();
    Json(ExistsResponse { present })
}

/// Streams the file at `path` as the response body. Returns 404 if missing.
async fn stream_file(path: &StdPath) -> Result<Response, StatusCode> {
    let file = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(StatusCode::NOT_FOUND),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let meta = file.metadata().await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_LENGTH, meta.len().into());
    headers.insert(header::CONTENT_TYPE, header::HeaderValue::from_static("application/octet-stream"));

    let mut resp = Response::new(body);
    *resp.status_mut() = StatusCode::OK;
    *resp.headers_mut() = headers;
    Ok(resp)
}

/// Streams `body` into `final_path` via a temp file. If `expected_hash` is
/// Some, computes blake3 as it writes and rejects with 400 on mismatch. On
/// success the temp file is atomically renamed onto `final_path`.
async fn stream_in_verified(final_path: &StdPath, body: Body, expected_hash: Option<&str>) -> Result<StatusCode, StatusCode> {
    if let Some(parent) = final_path.parent() {
        if let Err(_) = tokio::fs::create_dir_all(parent).await {
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    let tmp_path = {
        let mut p = final_path.as_os_str().to_owned();
        p.push(".tmp");
        PathBuf::from(p)
    };

    let mut file = match File::create(&tmp_path).await {
        Ok(f) => f,
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    };

    let mut hasher = expected_hash.map(|_| blake3::Hasher::new());
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk: Bytes = match chunk {
            Ok(c) => c,
            Err(_) => {
                let _ = tokio::fs::remove_file(&tmp_path).await;
                return Err(StatusCode::BAD_REQUEST);
            }
        };
        if let Some(h) = hasher.as_mut() {
            h.update(&chunk);
        }
        if file.write_all(&chunk).await.is_err() {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    }

    if file.flush().await.is_err() || file.sync_all().await.is_err() {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    drop(file);

    if let (Some(h), Some(expected)) = (hasher, expected_hash) {
        let computed = h.finalize().to_hex().to_string();
        if computed != expected {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(StatusCode::BAD_REQUEST);
        }
    }

    if tokio::fs::rename(&tmp_path, final_path).await.is_err() {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok(StatusCode::OK)
}

#[derive(Debug, Serialize)]
struct GcReport {
    blobs_scanned: usize,
    blobs_deleted: usize,
    thumbnails_scanned: usize,
    thumbnails_deleted: usize,
    bytes_reclaimed: u64,
}

/// Delete blob and thumbnail files not referenced by any live `book_dir_added`
/// event. Files modified within the safety window (default 1 hour) are skipped
/// to avoid racing in-flight uploads whose creating event hasn't been pushed yet.
async fn post_admin_gc(State(state): State<AppState>) -> Result<Json<GcReport>, StatusCode> {
    let live = db::live_blob_hashes(&state.pool).await.map_err(|err| {
        eprintln!("gc: live_blob_hashes failed: {err:?}");
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    let safety_window = std::time::Duration::from_secs(3600);
    let cutoff = std::time::SystemTime::now()
        .checked_sub(safety_window)
        .unwrap_or(std::time::UNIX_EPOCH);

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
async fn sweep_dir(
    root: &StdPath,
    live: &std::collections::HashSet<String>,
    cutoff: std::time::SystemTime,
    sharded: bool,
) -> (usize, usize, u64) {
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
                let p = file.path();
                let (s, d, b) = consider_file(&p, live, cutoff).await;
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

async fn consider_file(
    path: &StdPath,
    live: &std::collections::HashSet<String>,
    cutoff: std::time::SystemTime,
) -> (usize, usize, u64) {
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

// ---- auth ----

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

fn auth_error_to_status(err: AuthError) -> StatusCode {
    match err {
        AuthError::InvalidCredentials | AuthError::InvalidInvite => StatusCode::UNAUTHORIZED,
        AuthError::UsernameTaken | AuthError::UserIdTaken => StatusCode::CONFLICT,
        AuthError::Forbidden => StatusCode::FORBIDDEN,
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

#[derive(Debug, Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Debug, Deserialize)]
struct SignupRequest {
    username: String,
    password: String,
    invite: String,
    #[serde(default)]
    adopt_user_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct AuthResponse {
    token: String,
    user_id: String,
    username: String,
}

#[derive(Debug, Serialize)]
struct MeResponse {
    user_id: String,
    username: String,
}

async fn post_login(State(state): State<AppState>, Json(req): Json<LoginRequest>) -> Result<Json<AuthResponse>, StatusCode> {
    let (token, SessionRecord { user_id, username }) =
        auth::login(&state.pool, &req.username, &req.password).await.map_err(auth_error_to_status)?;
    Ok(Json(AuthResponse { token, user_id, username }))
}

async fn post_logout(State(state): State<AppState>, user: AuthUser, parts: axum::http::HeaderMap) -> Result<StatusCode, StatusCode> {
    // We have the user from extraction; pull the raw token to invalidate it.
    let header = parts.get(axum::http::header::AUTHORIZATION).and_then(|v| v.to_str().ok());
    if let Some(h) = header {
        if let Some(token) = h.strip_prefix("Bearer ") {
            let _ = auth::logout(&state.pool, token.trim()).await;
        }
    }
    let _ = user;
    Ok(StatusCode::NO_CONTENT)
}

async fn post_signup(State(state): State<AppState>, Json(req): Json<SignupRequest>) -> Result<Json<AuthResponse>, StatusCode> {
    let (token, SessionRecord { user_id, username }) = auth::signup(
        &state.pool,
        &req.invite,
        &req.username,
        &req.password,
        req.adopt_user_id.as_deref(),
    )
    .await
    .map_err(auth_error_to_status)?;
    Ok(Json(AuthResponse { token, user_id, username }))
}

async fn get_me(user: AuthUser) -> Json<MeResponse> {
    Json(MeResponse { user_id: user.user_id, username: user.username })
}

#[derive(Debug, Deserialize, Default)]
struct CreateInviteRequest {
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    expires_in_days: Option<i64>,
}

#[derive(Debug, Serialize)]
struct CreateInviteResponse {
    invite: String,
    expires_at: chrono::DateTime<chrono::Utc>,
}

async fn post_admin_invite(
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
