//! Axum transport adapter for the synchronization server.

mod admin;
mod assets;
mod bandwidth;
mod extract;
mod notifications;
mod preferences;
mod state;
mod sync;
mod api_wire;

use axum::{
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    routing::{get, head, patch, post},
    Router,
};
use server_account_core::IdentityResolver;
use server_core::{AssetService, GarbageCollector, PreferenceService, StorageQuotaService, SyncService};
use state::AppState;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::timeout::{RequestBodyTimeoutLayer, TimeoutLayer};

pub use bandwidth::TransferLimits;
pub use notifications::NotificationConfig;

const MAX_REQUEST_BODY_BYTES: usize = 8 * 1024 * 1024;

#[axum::async_trait]
pub trait ReadinessProbe: Send + Sync {
    async fn check(&self) -> Result<(), String>;
}

/// Coalesces concurrent readiness requests and briefly reuses their result.
///
/// A readiness check may include an actual durable write to the asset volume.
/// Caching keeps a public monitoring endpoint from turning that useful check
/// into an unbounded stream of filesystem syncs.
pub struct CachedReadiness {
    probe: Arc<dyn ReadinessProbe>,
    ttl: Duration,
    cached: Mutex<Option<(Instant, Result<(), String>)>>,
}

impl CachedReadiness {
    pub fn new(probe: Arc<dyn ReadinessProbe>, ttl: Duration) -> Self {
        Self { probe, ttl, cached: Mutex::new(None) }
    }
}

#[axum::async_trait]
impl ReadinessProbe for CachedReadiness {
    async fn check(&self) -> Result<(), String> {
        let mut cached = self.cached.lock().await;
        if let Some((checked_at, result)) = cached.as_ref() {
            if checked_at.elapsed() < self.ttl {
                return result.clone();
            }
        }

        let result = self.probe.check().await;
        *cached = Some((Instant::now(), result.clone()));
        result
    }
}

struct AlwaysReady;

#[axum::async_trait]
impl ReadinessProbe for AlwaysReady {
    async fn check(&self) -> Result<(), String> {
        Ok(())
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
        let max_in_flight = parse_bounded_usize("BOKHEIM_MAX_IN_FLIGHT_REQUESTS", defaults.max_in_flight, 1, 4096)?;
        let timeout_seconds = parse_bounded_u64("BOKHEIM_REQUEST_TIMEOUT_SECONDS", defaults.timeout.as_secs(), 30, 24 * 60 * 60)?;
        let body_idle_seconds = parse_bounded_u64("BOKHEIM_BODY_IDLE_TIMEOUT_SECONDS", defaults.body_idle_timeout.as_secs(), 5, 60 * 60)?;
        Ok(Self { max_in_flight, timeout: Duration::from_secs(timeout_seconds), body_idle_timeout: Duration::from_secs(body_idle_seconds) })
    }

    pub fn apply(self, router: Router) -> Router {
        // Axum's last applied layer is outermost. Keep the total deadline
        // outside the concurrency limiter so waiting for capacity is bounded
        // too, rather than only timing requests after they acquire a permit.
        router.layer(ConcurrencyLimitLayer::new(self.max_in_flight)).layer(RequestBodyTimeoutLayer::new(self.body_idle_timeout)).layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, self.timeout))
    }
}

fn parse_bounded_usize(name: &str, default: usize, minimum: usize, maximum: usize) -> Result<usize, String> {
    let value = match std::env::var(name) {
        Ok(raw) => raw.parse().map_err(|_| format!("{name} must be an integer"))?,
        Err(std::env::VarError::NotPresent) => default,
        Err(error) => return Err(format!("cannot read {name}: {error}")),
    };
    (minimum..=maximum).contains(&value).then_some(value).ok_or_else(|| format!("{name} must be between {minimum} and {maximum}"))
}

fn parse_bounded_u64(name: &str, default: u64, minimum: u64, maximum: u64) -> Result<u64, String> {
    let value = match std::env::var(name) {
        Ok(raw) => raw.parse().map_err(|_| format!("{name} must be an integer"))?,
        Err(std::env::VarError::NotPresent) => default,
        Err(error) => return Err(format!("cannot read {name}: {error}")),
    };
    (minimum..=maximum).contains(&value).then_some(value).ok_or_else(|| format!("{name} must be between {minimum} and {maximum}"))
}

#[derive(Clone)]
pub struct AdministrationServices {
    pub(crate) storage_quotas: Arc<dyn StorageQuotaService>,
    pub(crate) garbage_collector: Arc<GarbageCollector>,
    pub(crate) readiness: Arc<dyn ReadinessProbe>,
}

impl AdministrationServices {
    pub fn new(storage_quotas: Arc<dyn StorageQuotaService>, garbage_collector: Arc<GarbageCollector>) -> Self {
        Self { storage_quotas, garbage_collector, readiness: Arc::new(AlwaysReady) }
    }

    pub fn with_readiness(storage_quotas: Arc<dyn StorageQuotaService>, garbage_collector: Arc<GarbageCollector>, readiness: Arc<dyn ReadinessProbe>) -> Self {
        Self { storage_quotas, garbage_collector, readiness }
    }
}

pub fn router(identities: Arc<dyn IdentityResolver>, sync: Arc<dyn SyncService>, preferences: Arc<dyn PreferenceService>, assets: Arc<dyn AssetService>, administration: AdministrationServices) -> Router {
    router_with_transfer_limits(identities, sync, preferences, assets, administration, TransferLimits::default())
}

pub fn router_with_transfer_limits(
    identities: Arc<dyn IdentityResolver>, sync: Arc<dyn SyncService>, preferences: Arc<dyn PreferenceService>, assets: Arc<dyn AssetService>, administration: AdministrationServices, transfer_limits: TransferLimits,
) -> Router {
    router_with_operational_limits(identities, sync, preferences, assets, administration, transfer_limits, NotificationConfig::default())
}

pub fn router_with_operational_limits(
    identities: Arc<dyn IdentityResolver>, sync: Arc<dyn SyncService>, preferences: Arc<dyn PreferenceService>, assets: Arc<dyn AssetService>, administration: AdministrationServices, transfer_limits: TransferLimits,
    notification_config: NotificationConfig,
) -> Router {
    let state = AppState::new(identities, sync, preferences, assets, administration, transfer_limits, notification_config);
    Router::new()
        .route("/api/live", get(live))
        .route("/api/ready", get(ready))
        .route("/api/health", get(ready))
        .route("/api/notifications", get(notifications::connect))
        .route("/api/sync/push", post(sync::push_events))
        .route("/api/sync/pull", get(sync::pull_events))
        .route("/api/sync/exchange", post(sync::exchange_events))
        .route("/api/libraries", get(sync::list_libraries).post(sync::upsert_library))
        .route("/api/preferences/initialize", post(preferences::initialize))
        .route("/api/preferences", patch(preferences::apply))
        .route("/api/libraries/:library_id/blobs/:hash", head(assets::head_blob).get(assets::get_blob).put(assets::put_blob))
        .route("/api/libraries/:library_id/blobs/exists", post(assets::post_blobs_exists))
        .route("/api/libraries/:library_id/blobs/negotiate", post(assets::post_blob_manifest))
        .route("/api/libraries/:library_id/thumbnails/:content_hash", head(assets::head_thumbnail).get(assets::get_thumbnail).put(assets::put_thumbnail))
        .route("/api/libraries/:library_id/thumbnails/exists", post(assets::post_thumbnails_exists))
        .route("/api/admin/users/:user_id/storage-quota", post(admin::post_admin_storage_quota))
        .route("/api/admin/gc", post(admin::post_admin_gc))
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

#[cfg(test)]
mod tests {
    use super::{parse_bounded_u64, parse_bounded_usize, readiness_response, CachedReadiness, ReadinessProbe, RequestLimits};
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
        assert!(parse_bounded_usize("UNSET_BOKHEIM_TEST_LIMIT", 12, 1, 20).is_ok());
        assert!(parse_bounded_u64("UNSET_BOKHEIM_TEST_TIMEOUT", 30, 30, 60).is_ok());
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
