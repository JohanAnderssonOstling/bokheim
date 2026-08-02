use crate::extract::{sync_error, AuthUser};
use crate::state::AppState;
use axum::extract::{Json, Query, State};
use sync_common::api::libraries::{LibrarySummary, UpsertLibraryRequest};
use sync_common::{PullChangesResponse, PullQuery, PushChangesRequest, PushChangesResponse};

pub(super) async fn list_libraries(State(state): State<AppState>, user: AuthUser) -> Result<Json<Vec<LibrarySummary>>, axum::http::StatusCode> {
    state.sync.list_libraries(&user.user_id).await.map(Json).map_err(sync_error)
}

pub(super) async fn upsert_library(State(state): State<AppState>, user: AuthUser, Json(request): Json<UpsertLibraryRequest>) -> Result<Json<LibrarySummary>, axum::http::StatusCode> {
    state.sync.upsert_library(&user.user_id, request).await.map(Json).map_err(sync_error)
}

pub(super) async fn push_events(State(state): State<AppState>, user: AuthUser, Json(request): Json<PushChangesRequest>) -> Result<Json<PushChangesResponse>, axum::http::StatusCode> {
    state.sync.push(&user.user_id, request).await.map(Json).map_err(sync_error)
}

pub(super) async fn pull_events(State(state): State<AppState>, user: AuthUser, Query(query): Query<PullQuery>) -> Result<Json<PullChangesResponse>, axum::http::StatusCode> {
    state.sync.pull(&user.user_id, query).await.map(Json).map_err(sync_error)
}
