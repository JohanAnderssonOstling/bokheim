//! Axum transport adapter for the server application.

mod admin;
mod assets;
mod auth;
mod bandwidth;
mod extract;
mod opds;
mod state;
mod sync;

use axum::{
    extract::DefaultBodyLimit,
    http::StatusCode,
    routing::{get, head, post},
    Router,
};
use server_core::{AssetService, AuthService, GarbageCollector, StorageQuotaService, SyncService};
use state::AppState;
use std::sync::Arc;
use sync_common::api::opds::{CatalogRegistry, CATALOG_REGISTRY_SCHEMA_VERSION};

pub use bandwidth::TransferLimits;

const MAX_JSON_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_AUTH_BODY_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientAddressSource {
    DirectPeer,
    TrustedForwardedFor,
}

pub fn router(auth: Arc<dyn AuthService>, sync: Arc<dyn SyncService>, assets: Arc<dyn AssetService>, storage_quotas: Arc<dyn StorageQuotaService>, garbage_collector: Arc<GarbageCollector>) -> Router {
    router_with_client_address_source(auth, sync, assets, storage_quotas, garbage_collector, ClientAddressSource::DirectPeer)
}

pub fn router_with_client_address_source(
    auth: Arc<dyn AuthService>, sync: Arc<dyn SyncService>, assets: Arc<dyn AssetService>, storage_quotas: Arc<dyn StorageQuotaService>, garbage_collector: Arc<GarbageCollector>, client_address_source: ClientAddressSource,
) -> Router {
    router_with_client_address_source_and_transfer_limits(auth, sync, assets, storage_quotas, garbage_collector, client_address_source, TransferLimits::default())
}

pub fn router_with_client_address_source_and_transfer_limits(
    auth: Arc<dyn AuthService>, sync: Arc<dyn SyncService>, assets: Arc<dyn AssetService>, storage_quotas: Arc<dyn StorageQuotaService>, garbage_collector: Arc<GarbageCollector>, client_address_source: ClientAddressSource,
    transfer_limits: TransferLimits,
) -> Router {
    router_with_catalog_registry(auth, sync, assets, storage_quotas, garbage_collector, client_address_source, transfer_limits, CatalogRegistry { schema_version: CATALOG_REGISTRY_SCHEMA_VERSION, revision: 0, catalogs: Vec::new() })
}

pub fn router_with_catalog_registry(auth: Arc<dyn AuthService>, sync: Arc<dyn SyncService>, assets: Arc<dyn AssetService>, storage_quotas: Arc<dyn StorageQuotaService>, garbage_collector: Arc<GarbageCollector>, client_address_source: ClientAddressSource, transfer_limits: TransferLimits, opds_catalogs: CatalogRegistry) -> Router {
    let state = AppState::new(auth, sync, assets, storage_quotas, garbage_collector, client_address_source, transfer_limits, Arc::new(opds_catalogs));
    Router::new()
        .route("/api/health", get(health))
        .route("/api/opds/catalogs", get(opds::get_catalogs))
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
        .route("/api/admin/users/:user_id/storage-quota", post(admin::post_admin_storage_quota).layer(DefaultBodyLimit::max(MAX_AUTH_BODY_BYTES)))
        .route("/api/admin/gc", post(admin::post_admin_gc))
        .layer(DefaultBodyLimit::max(MAX_JSON_BODY_BYTES))
        .with_state(state)
}

async fn health() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}
