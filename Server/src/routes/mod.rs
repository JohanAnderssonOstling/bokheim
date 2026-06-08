mod admin;
mod auth;
mod blobs;
mod extract;
mod sync;

use axum::{
    extract::DefaultBodyLimit,
    http::StatusCode,
    routing::{get, head, post},
    Router,
};
use sqlx::PgPool;
use std::path::PathBuf;

use crate::state::AppState;

const MAX_UPLOAD_BYTES: usize = 1024 * 1024 * 1024; // 1 GiB

pub fn router(pool: PgPool) -> Router {
    let books_dir = PathBuf::from("./books");
    let thumbnails_dir = PathBuf::from("./thumbnails");
    let _ = std::fs::create_dir_all(&books_dir);
    let _ = std::fs::create_dir_all(&thumbnails_dir);
    let state = AppState::new(pool, books_dir, thumbnails_dir);

    Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/signup", post(auth::post_signup))
        .route("/api/auth/login", post(auth::post_login))
        .route("/api/auth/logout", post(auth::post_logout))
        .route("/api/auth/me", get(auth::get_me))
        .route("/api/sync/push", post(sync::push_events))
        .route("/api/sync/pull", get(sync::pull_events))
        .route("/api/blobs/:hash", head(blobs::head_blob).get(blobs::get_blob).put(blobs::put_blob))
        .route("/api/blobs/exists", post(blobs::post_blobs_exists))
        .route(
            "/api/thumbnails/:content_hash",
            head(blobs::head_thumbnail_blob).get(blobs::get_thumbnail_blob).put(blobs::put_thumbnail_blob),
        )
        .route("/api/thumbnails/exists", post(blobs::post_thumbnails_exists))
        .route("/api/admin/invites", post(admin::post_admin_invite))
        .route("/api/admin/gc", post(admin::post_admin_gc))
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES))
        .with_state(state)
}

async fn health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}
