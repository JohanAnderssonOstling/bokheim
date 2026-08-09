//! Axum transport adapter for the synchronization server.

mod api_wire;
mod assets;
mod bandwidth;
mod extract;
mod notifications;
mod state;
mod sync;

use axum::{
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    routing::{get, head, post},
    Router,
};
use server_account::PostgresAccountService;
use server_asset_store::FsAssetStore;
use server_database::PostgresDatabase;
use server_postgres::{AssetApplication, PostgresSyncRepository};
use state::AppState;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::timeout::{RequestBodyTimeoutLayer, TimeoutLayer};

pub use bandwidth::TransferLimits;
pub use notifications::NotificationConfig;

const MAX_REQUEST_BODY_BYTES: usize = 8 * 1024 * 1024;

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
            tokio::time::timeout(Duration::from_secs(3), server_database::check_ready(&self.database)).await.map_err(|_| "database readiness timed out".to_string())?.map_err(|error| format!("database: {error}"))?;
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
    timeout: Duration,
    body_idle_timeout: Duration,
}

impl Default for RequestLimits {
    fn default() -> Self {
        Self { max_in_flight: 512, timeout: Duration::from_secs(2 * 60 * 60), body_idle_timeout: Duration::from_secs(60) }
    }
}

impl RequestLimits {
    pub fn from_env() -> Result<Self, String> {
        let defaults = Self::default();
        let max_in_flight = parse_bounded("BOKHEIM_MAX_IN_FLIGHT_REQUESTS", defaults.max_in_flight, 1, 4096)?;
        let timeout_seconds = parse_bounded("BOKHEIM_REQUEST_TIMEOUT_SECONDS", defaults.timeout.as_secs(), 30, 24 * 60 * 60)?;
        let body_idle_seconds = parse_bounded("BOKHEIM_BODY_IDLE_TIMEOUT_SECONDS", defaults.body_idle_timeout.as_secs(), 5, 60 * 60)?;
        Ok(Self { max_in_flight, timeout: Duration::from_secs(timeout_seconds), body_idle_timeout: Duration::from_secs(body_idle_seconds) })
    }

    pub fn apply(self, router: Router) -> Router {
        // Axum's last applied layer is outermost. Keep the total deadline
        // outside the concurrency limiter so waiting for capacity is bounded
        // too, rather than only timing requests after they acquire a permit.
        router.layer(ConcurrencyLimitLayer::new(self.max_in_flight)).layer(RequestBodyTimeoutLayer::new(self.body_idle_timeout)).layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, self.timeout))
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
        .route("/api/sync/exchange", post(sync::exchange_events))
        .route("/api/libraries", get(sync::list_libraries).post(sync::upsert_library))
        .route("/api/libraries/:library_id/blobs/:hash", head(assets::head_blob).get(assets::get_blob).put(assets::put_blob))
        .route("/api/libraries/:library_id/blobs/negotiate", post(assets::post_blob_manifest))
        .route("/api/libraries/:library_id/thumbnails/:content_hash", head(assets::head_thumbnail).get(assets::get_thumbnail).put(assets::put_thumbnail))
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

#[cfg(any())]
mod tests {
    use super::{parse_bounded, readiness_response, CachedReadiness, ReadinessProbe, RequestLimits};
    use axum::{
        extract::State,
        http::{Request, StatusCode},
        routing::get,
        Router,
    };
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;
    use tokio::sync::Notify;
    use tower::ServiceExt;

    struct CountingReadiness {
        checks: Arc<AtomicUsize>,
    }

    #[axum::async_trait]
    impl ReadinessProbe for CountingReadiness {
        async fn check(&self) -> Result<(), String> {
            self.checks.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn operational_limit_defaults_are_bounded() {
        let limits = RequestLimits::default();
        assert_eq!(limits.max_in_flight, 512);
        assert_eq!(limits.timeout.as_secs(), 7200);
        assert_eq!(limits.body_idle_timeout.as_secs(), 60);
        assert!(parse_bounded::<usize>("UNSET_BOKHEIM_TEST_LIMIT", 12, 1, 20).is_ok());
        assert!(parse_bounded::<u64>("UNSET_BOKHEIM_TEST_TIMEOUT", 30, 30, 60).is_ok());
    }

    #[test]
    fn readiness_failure_is_not_reported_as_a_healthy_process() {
        assert_eq!(readiness_response(Ok(())), (StatusCode::OK, "ok"));
        assert_eq!(readiness_response(Err("database unavailable".to_string())), (StatusCode::SERVICE_UNAVAILABLE, "not ready"));
    }

    #[tokio::test]
    async fn readiness_checks_are_reused_within_the_cache_window() {
        let checks = Arc::new(AtomicUsize::new(0));
        let probe: Arc<dyn ReadinessProbe> = Arc::new(CountingReadiness { checks: checks.clone() });
        let cached = CachedReadiness::new(probe, Duration::from_secs(5));

        cached.check().await.unwrap();
        cached.check().await.unwrap();

        assert_eq!(checks.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn total_deadline_includes_time_waiting_for_concurrency() {
        #[derive(Clone)]
        struct Gate {
            entered: Arc<Notify>,
            release: Arc<Notify>,
        }

        async fn blocked(State(gate): State<Gate>) -> StatusCode {
            gate.entered.notify_one();
            gate.release.notified().await;
            StatusCode::OK
        }

        let gate = Gate { entered: Arc::new(Notify::new()), release: Arc::new(Notify::new()) };
        let limits = RequestLimits { max_in_flight: 1, timeout: Duration::from_millis(50), body_idle_timeout: Duration::from_secs(1) };
        let app = limits.apply(Router::new().route("/", get(blocked)).with_state(gate.clone()));
        let first = tokio::spawn(app.clone().oneshot(Request::get("/").body(axum::body::Body::empty()).unwrap()));
        gate.entered.notified().await;

        let second = tokio::time::timeout(Duration::from_millis(500), app.oneshot(Request::get("/").body(axum::body::Body::empty()).unwrap())).await.expect("the outer deadline must bound the concurrency queue").unwrap();
        assert_eq!(second.status(), StatusCode::REQUEST_TIMEOUT);

        gate.release.notify_waiters();
        let _ = first.await;
    }
}
