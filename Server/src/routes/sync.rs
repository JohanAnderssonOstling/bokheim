use axum::extract::{Json, Query, State};
use axum::http::StatusCode;

use crate::db;
use crate::routes::extract::AuthUser;
use crate::state::AppState;
use sync_common::{PullChangesResponse, PullQuery, PushChangesRequest, PushChangesResponse};

pub async fn push_events(State(state): State<AppState>, user: AuthUser, Json(payload): Json<PushChangesRequest>) -> Result<Json<PushChangesResponse>, StatusCode> {
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

pub async fn pull_events(State(state): State<AppState>, user: AuthUser, Query(query): Query<PullQuery>) -> Result<Json<PullChangesResponse>, StatusCode> {
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
