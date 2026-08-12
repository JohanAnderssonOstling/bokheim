//! Axum transport adapter for the synchronization server.

mod account;
mod assets;
mod bandwidth;
mod extract;
mod notifications;
mod state;
mod sync;

use axum::{
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    routing::{delete, get, head, post},
    Router,
};
use server_account::PostgresAccountService;
use server_asset_store::FsAssetStore;
use server_postgres::{AssetApplication, PostgresDatabase, PostgresSyncRepository};
use state::AppState;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::timeout::{RequestBodyTimeoutLayer, TimeoutLayer};

pub use bandwidth::TransferLimits;
pub use notifications::NotificationConfig;

const MAX_REQUEST_BODY_BYTES: usize = 8 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = server_asset_store::MAX_BOOK_UPLOAD_DURATION;
const REQUEST_BODY_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Coalesces concurrent readiness requests and briefly reuses their result.
///
/// A readiness check may include an actual durable write to the asset volume.
/// Caching keeps a public monitoring endpoint from turning that useful check
/// into an unbounded stream of filesystem syncs.
pub struct Readiness {
    database: PostgresDatabase,
    assets: Arc<FsAssetStore>,
    ttl: Duration,
    cached: Mutex<Option<(Instant, Result<(), String>)>>,
}

impl Readiness {
    pub fn new(database: PostgresDatabase, assets: Arc<FsAssetStore>, ttl: Duration) -> Self {
        Self { database, assets, ttl, cached: Mutex::new(None) }
    }

    pub async fn check(&self) -> Result<(), String> {
        let mut cached = self.cached.lock().await;
        if let Some((checked_at, result)) = cached.as_ref() {
            if checked_at.elapsed() < self.ttl {
                return result.clone();
            }
        }

        let result = async {
            tokio::time::timeout(Duration::from_secs(3), server_postgres::check_ready(&self.database)).await.map_err(|_| "database readiness timed out".to_string())?.map_err(|error| format!("database: {error}"))?;
            tokio::time::timeout(Duration::from_secs(3), self.assets.check_ready()).await.map_err(|_| "asset storage readiness timed out".to_string())?.map_err(|error| format!("asset storage: {error}"))
        }
        .await;
        *cached = Some((Instant::now(), result.clone()));
        result
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestLimits {
    max_in_flight: usize,
}

impl Default for RequestLimits {
    fn default() -> Self {
        Self { max_in_flight: 512 }
    }
}

impl RequestLimits {
    pub fn from_env() -> Result<Self, String> {
        let defaults = Self::default();
        let max_in_flight = parse_bounded("BOKHEIM_MAX_IN_FLIGHT_REQUESTS", defaults.max_in_flight, 1, 4096)?;
        Ok(Self { max_in_flight })
    }

    pub fn apply(self, router: Router) -> Router {
        // Axum's last applied layer is outermost. Keep the total deadline
        // outside the concurrency limiter so waiting for capacity is bounded
        // too, rather than only timing requests after they acquire a permit.
        router.layer(ConcurrencyLimitLayer::new(self.max_in_flight)).layer(RequestBodyTimeoutLayer::new(REQUEST_BODY_IDLE_TIMEOUT)).layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, REQUEST_TIMEOUT))
    }
}

fn parse_bounded<T: std::str::FromStr + PartialOrd + Copy + std::fmt::Display>(name: &str, default: T, minimum: T, maximum: T) -> Result<T, String> {
    let value = match std::env::var(name) {
        Ok(raw) => raw.parse().map_err(|_| format!("{name} must be an integer"))?,
        Err(std::env::VarError::NotPresent) => default,
        Err(error) => return Err(format!("cannot read {name}: {error}")),
    };
    (minimum..=maximum).contains(&value).then_some(value).ok_or_else(|| format!("{name} must be between {minimum} and {maximum}"))
}

pub fn router_with_operational_limits(
    account: Arc<PostgresAccountService>, sync: Arc<PostgresSyncRepository>, assets: Arc<AssetApplication>, readiness: Arc<Readiness>, transfer_limits: TransferLimits, notification_config: NotificationConfig,
) -> Router {
    let state = AppState::new(account, sync, assets, readiness, transfer_limits, notification_config);
    Router::new()
        .route("/api/live", get(live))
        .route("/api/ready", get(ready))
        .route("/api/health", get(ready))
        .route("/api/notifications", get(notifications::connect))
        .route("/api/account/storage", get(account::get_storage_usage))
        .route("/api/account/storage/libraries", get(account::get_library_storage_usage))
        .route("/api/sync/exchange", post(sync::exchange_events))
        .route("/api/sync/inventory", post(sync::compare_state_inventory))
        .route("/api/libraries", get(sync::list_libraries).post(sync::upsert_library))
        .route("/api/libraries/deleted", get(sync::list_deleted_libraries))
        .route("/api/libraries/:library_id", delete(sync::delete_library))
        .route("/api/libraries/:library_id/blobs/:hash", head(assets::head_blob).get(assets::get_blob).put(assets::put_blob))
        .route("/api/libraries/:library_id/blobs/negotiate", post(assets::post_blob_manifest))
        .route("/api/libraries/:library_id/thumbnails/:content_hash", head(assets::head_thumbnail).get(assets::get_thumbnail).put(assets::put_thumbnail))
        .route("/api/libraries/:library_id/thumbnail-batch", post(assets::post_thumbnail_batch).put(assets::put_thumbnail_batch).layer(DefaultBodyLimit::max(sync_common::api::assets::MAX_THUMBNAIL_BATCH_BYTES + 1024 * 1024)))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BODY_BYTES))
        .with_state(state)
}

async fn live() -> (StatusCode, &'static str) {
    (StatusCode::OK, "ok")
}

async fn ready(State(state): State<AppState>) -> (StatusCode, &'static str) {
    readiness_response(state.readiness.check().await)
}

fn readiness_response(result: Result<(), String>) -> (StatusCode, &'static str) {
    match result {
        Ok(()) => (StatusCode::OK, "ok"),
        Err(reason) => {
            tracing::error!(%reason, "readiness probe failed");
            (StatusCode::SERVICE_UNAVAILABLE, "not ready")
        }
    }
}
