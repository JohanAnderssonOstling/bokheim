//! Account-wide capacity checks for changes to library storage policy.
use tokio::sync::OwnedMutexGuard;

pub struct CloudQuotaPermit {
    _guard: OwnedMutexGuard<()>,
}

impl CloudQuotaPermit {
    pub fn new(guard: OwnedMutexGuard<()>) -> Self {
        Self { _guard: guard }
    }
}
