use crate::bandwidth::{TransferGovernor, TransferLimits};
use crate::notifications::{NotificationConfig, NotificationHub};
use crate::Readiness;
use server_account::PostgresAccountService;
use server_postgres::{AssetApplication, PostgresSyncRepository};
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) account: Arc<PostgresAccountService>,
    pub(crate) sync: Arc<PostgresSyncRepository>,
    pub(crate) assets: Arc<AssetApplication>,
    pub(crate) readiness: Arc<Readiness>,
    pub(crate) transfer_governor: TransferGovernor,
    pub(crate) notifications: NotificationHub,
}

impl AppState {
    pub(super) fn new(account: Arc<PostgresAccountService>, sync: Arc<PostgresSyncRepository>, assets: Arc<AssetApplication>, readiness: Arc<Readiness>, transfer_limits: TransferLimits, notification_config: NotificationConfig) -> Self {
        Self { account, sync, assets, readiness, transfer_governor: TransferGovernor::new(transfer_limits), notifications: NotificationHub::new(notification_config) }
    }
}
