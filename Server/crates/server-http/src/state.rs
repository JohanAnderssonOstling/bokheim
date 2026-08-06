use crate::bandwidth::{TransferGovernor, TransferLimits};
use crate::notifications::{NotificationConfig, NotificationHub};
use crate::{AdministrationServices, ReadinessProbe};
use server_account_core::IdentityResolver;
use server_core::{AssetService, GarbageCollector, PreferenceService, StorageQuotaService, SyncService};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) identities: Arc<dyn IdentityResolver>,
    pub(crate) sync: Arc<dyn SyncService>,
    pub(crate) preferences: Arc<dyn PreferenceService>,
    pub(crate) assets: Arc<dyn AssetService>,
    pub(crate) storage_quotas: Arc<dyn StorageQuotaService>,
    pub(crate) garbage_collector: Arc<GarbageCollector>,
    pub(crate) readiness: Arc<dyn ReadinessProbe>,
    pub(crate) transfer_governor: TransferGovernor,
    pub(crate) notifications: NotificationHub,
}

impl AppState {
    pub(super) fn new(
        identities: Arc<dyn IdentityResolver>, sync: Arc<dyn SyncService>, preferences: Arc<dyn PreferenceService>, assets: Arc<dyn AssetService>, administration: AdministrationServices, transfer_limits: TransferLimits,
        notification_config: NotificationConfig,
    ) -> Self {
        Self {
            identities,
            sync,
            preferences,
            assets,
            storage_quotas: administration.storage_quotas,
            garbage_collector: administration.garbage_collector,
            readiness: administration.readiness,
            transfer_governor: TransferGovernor::new(transfer_limits),
            notifications: NotificationHub::new(notification_config),
        }
    }
}
