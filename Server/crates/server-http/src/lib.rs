//! Axum transport adapter for the server application.

mod admin;
mod assets;
mod auth;
mod bandwidth;
mod extract;
mod state;
mod sync;

use axum::{
    extract::DefaultBodyLimit,
    http::StatusCode,
    routing::{get, head, post},
    Router,
};
use server_core::{AssetService, AuthService, GarbageCollector, SyncService};
use state::AppState;
use std::sync::Arc;

pub use bandwidth::TransferLimits;

const MAX_JSON_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_AUTH_BODY_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientAddressSource {
    DirectPeer,
    TrustedForwardedFor,
}

pub fn router(auth: Arc<dyn AuthService>, sync: Arc<dyn SyncService>, assets: Arc<dyn AssetService>, garbage_collector: Arc<GarbageCollector>) -> Router {
    router_with_client_address_source(auth, sync, assets, garbage_collector, ClientAddressSource::DirectPeer)
}

pub fn router_with_client_address_source(auth: Arc<dyn AuthService>, sync: Arc<dyn SyncService>, assets: Arc<dyn AssetService>, garbage_collector: Arc<GarbageCollector>, client_address_source: ClientAddressSource) -> Router {
    router_with_client_address_source_and_transfer_limits(auth, sync, assets, garbage_collector, client_address_source, TransferLimits::default())
}

pub fn router_with_client_address_source_and_transfer_limits(
    auth: Arc<dyn AuthService>,
    sync: Arc<dyn SyncService>,
    assets: Arc<dyn AssetService>,
    garbage_collector: Arc<GarbageCollector>,
    client_address_source: ClientAddressSource,
    transfer_limits: TransferLimits,
) -> Router {
    let state = AppState::new(auth, sync, assets, garbage_collector, client_address_source, transfer_limits);
    Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/signup", post(auth::post_signup).layer(DefaultBodyLimit::max(MAX_AUTH_BODY_BYTES)))
        .route("/api/auth/login", post(auth::post_login).layer(DefaultBodyLimit::max(MAX_AUTH_BODY_BYTES)))
        .route("/api/auth/logout", post(auth::post_logout))
        .route("/api/auth/me", get(auth::get_me))
        .route("/api/sync/push", post(sync::push_events))
        .route("/api/sync/pull", get(sync::pull_events))
        .route("/api/libraries", get(sync::list_libraries).post(sync::upsert_library))
        .route("/api/libraries/:library_id/blobs/:hash", head(assets::head_blob).get(assets::get_blob).put(assets::put_blob))
        .route("/api/libraries/:library_id/blobs/exists", post(assets::post_blobs_exists))
        .route("/api/libraries/:library_id/thumbnails/:content_hash", head(assets::head_thumbnail).get(assets::get_thumbnail).put(assets::put_thumbnail))
        .route("/api/libraries/:library_id/thumbnails/exists", post(assets::post_thumbnails_exists))
        .route("/api/admin/invites", post(admin::post_admin_invite))
        .route("/api/admin/gc", post(admin::post_admin_gc))
        .layer(DefaultBodyLimit::max(MAX_JSON_BODY_BYTES))
        .with_state(state)
}

async fn health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}
