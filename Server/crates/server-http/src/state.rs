use crate::bandwidth::{TransferGovernor, TransferLimits};
use crate::ClientAddressSource;
use server_core::{AssetService, AuthService, GarbageCollector, StorageQuotaService, SyncService};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use sync_common::api::opds::CatalogRegistry;

const AUTH_ATTEMPTS_PER_WINDOW: u8 = 8;
const AUTH_ATTEMPT_WINDOW: Duration = Duration::from_secs(5 * 60);
const MAX_RATE_LIMIT_KEYS: usize = 100_000;

#[derive(Clone, Copy)]
struct AttemptWindow {
    started_at: Instant,
    attempts: u8,
}

pub(crate) struct AuthRateLimiter {
    attempts: std::sync::Mutex<HashMap<String, AttemptWindow>>,
}

impl AuthRateLimiter {
    fn new() -> Self {
        Self { attempts: std::sync::Mutex::new(HashMap::new()) }
    }

    /// Reserves an attempt before password hashing, which also caps concurrent
    /// attempts for a key instead of merely counting completed failures.
    pub(crate) fn begin(&self, key: String) -> bool {
        let Ok(mut attempts) = self.attempts.lock() else {
            return false;
        };
        let now = Instant::now();
        attempts.retain(|_, window| now.duration_since(window.started_at) < AUTH_ATTEMPT_WINDOW);
        if !attempts.contains_key(&key) && attempts.len() >= MAX_RATE_LIMIT_KEYS {
            return false;
        }
        let window = attempts.entry(key).or_insert(AttemptWindow { started_at: now, attempts: 0 });
        if window.attempts >= AUTH_ATTEMPTS_PER_WINDOW {
            return false;
        }
        window.attempts += 1;
        true
    }

    pub(crate) fn succeeded(&self, key: &str) {
        if let Ok(mut attempts) = self.attempts.lock() {
            attempts.remove(key);
        }
    }
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) auth: Arc<dyn AuthService>,
    pub(crate) sync: Arc<dyn SyncService>,
    pub(crate) assets: Arc<dyn AssetService>,
    pub(crate) storage_quotas: Arc<dyn StorageQuotaService>,
    pub(crate) garbage_collector: Arc<GarbageCollector>,
    pub(crate) auth_rate_limiter: Arc<AuthRateLimiter>,
    pub(crate) transfer_governor: TransferGovernor,
    pub(crate) client_address_source: ClientAddressSource,
    pub(crate) opds_catalogs: Arc<CatalogRegistry>,
}

impl AppState {
    pub(super) fn new(auth: Arc<dyn AuthService>, sync: Arc<dyn SyncService>, assets: Arc<dyn AssetService>, storage_quotas: Arc<dyn StorageQuotaService>, garbage_collector: Arc<GarbageCollector>, client_address_source: ClientAddressSource, transfer_limits: TransferLimits, opds_catalogs: Arc<CatalogRegistry>) -> Self {
        Self { auth, sync, assets, storage_quotas, garbage_collector, auth_rate_limiter: Arc::new(AuthRateLimiter::new()), transfer_governor: TransferGovernor::new(transfer_limits), client_address_source, opds_catalogs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authentication_attempts_are_bounded_and_success_resets_the_key() {
        let limiter = AuthRateLimiter::new();
        for _ in 0..AUTH_ATTEMPTS_PER_WINDOW {
            assert!(limiter.begin("login:127.0.0.1:reader".to_string()));
        }
        assert!(!limiter.begin("login:127.0.0.1:reader".to_string()));
        assert!(limiter.begin("login:127.0.0.1:other".to_string()));
        limiter.succeeded("login:127.0.0.1:reader");
        assert!(limiter.begin("login:127.0.0.1:reader".to_string()));
    }
}
