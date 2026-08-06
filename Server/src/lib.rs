pub mod storage_config;
pub mod transport;

use axum::Router;
use server_account_core::{AccountService, IdentityResolver};
use server_account_http::ClientAddressSource;
use server_account_postgres::PostgresAccountService;
use server_asset_store::FsAssetStore;
use server_core::{
    AssetApplication, AssetError, AssetRepository, AssetService, AssetStore, GarbageCollector, MaintenanceCoordinator, PreferenceApplication, PreferenceRepository, PreferenceService, StorageQuotaService, SyncApplication, SyncRepository,
    SyncService,
};
use server_database::PostgresDatabase;
use server_postgres::{PostgresPreferenceRepository, PostgresStorageQuotaService, PostgresSyncRepository};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const READINESS_COMPONENT_TIMEOUT: Duration = Duration::from_secs(3);
const READINESS_CACHE_TTL: Duration = Duration::from_secs(5);

pub async fn app(database: PostgresDatabase) -> Result<Router, AssetError> {
    app_with_client_address_source(database, ClientAddressSource::DirectPeer).await
}
pub async fn app_with_paths(database: PostgresDatabase, books: PathBuf, thumbnails: PathBuf) -> Result<Router, AssetError> {
    app_with_paths_and_client_address_source(database, books, thumbnails, ClientAddressSource::DirectPeer, server_http::TransferLimits::default()).await
}

pub async fn app_with_client_address_source(database: PostgresDatabase, client_address_source: ClientAddressSource) -> Result<Router, AssetError> {
    app_with_paths_and_client_address_source(database, PathBuf::from("./books"), PathBuf::from("./thumbnails"), client_address_source, server_http::TransferLimits::default()).await
}

pub async fn app_with_client_address_source_and_transfer_limits(database: PostgresDatabase, client_address_source: ClientAddressSource, transfer_limits: server_http::TransferLimits) -> Result<Router, AssetError> {
    let account = Arc::new(PostgresAccountService::new(database.clone()));
    app_with_configured_account(database, account, false, PathBuf::from("./books"), PathBuf::from("./thumbnails"), client_address_source, transfer_limits).await
}

async fn app_with_paths_and_client_address_source(database: PostgresDatabase, books: PathBuf, thumbnails: PathBuf, client_address_source: ClientAddressSource, transfer_limits: server_http::TransferLimits) -> Result<Router, AssetError> {
    let account = Arc::new(PostgresAccountService::new(database.clone()));
    app_with_configured_account(database, account, false, books, thumbnails, client_address_source, transfer_limits).await
}

pub async fn app_with_configured_account(
    database: PostgresDatabase, account: Arc<PostgresAccountService>, public_registration: bool, books: PathBuf, thumbnails: PathBuf, client_address_source: ClientAddressSource, transfer_limits: server_http::TransferLimits,
) -> Result<Router, AssetError> {
    app_with_configured_account_and_request_limits(database, account, public_registration, books, thumbnails, client_address_source, transfer_limits, server_http::RequestLimits::default()).await
}

pub async fn app_with_configured_account_and_request_limits(
    database: PostgresDatabase, account: Arc<PostgresAccountService>, public_registration: bool, books: PathBuf, thumbnails: PathBuf, client_address_source: ClientAddressSource, transfer_limits: server_http::TransferLimits,
    request_limits: server_http::RequestLimits,
) -> Result<Router, AssetError> {
    app_with_configured_operational_limits(database, account, public_registration, books, thumbnails, client_address_source, transfer_limits, request_limits, server_asset_store::ValidationLimits::default()).await
}

pub async fn app_with_configured_operational_limits(
    database: PostgresDatabase, account: Arc<PostgresAccountService>, public_registration: bool, books: PathBuf, thumbnails: PathBuf, client_address_source: ClientAddressSource, transfer_limits: server_http::TransferLimits,
    request_limits: server_http::RequestLimits, validation_limits: server_asset_store::ValidationLimits,
) -> Result<Router, AssetError> {
    app_with_all_operational_limits(database, account, public_registration, books, thumbnails, client_address_source, transfer_limits, request_limits, validation_limits, server_http::NotificationConfig::default()).await
}

pub async fn app_with_all_operational_limits(
    database: PostgresDatabase, account: Arc<PostgresAccountService>, public_registration: bool, books: PathBuf, thumbnails: PathBuf, client_address_source: ClientAddressSource, transfer_limits: server_http::TransferLimits,
    request_limits: server_http::RequestLimits, validation_limits: server_asset_store::ValidationLimits, notification_config: server_http::NotificationConfig,
) -> Result<Router, AssetError> {
    let assets: Arc<dyn AssetStore> = Arc::new(FsAssetStore::with_validation_limits(books, thumbnails, validation_limits));
    assets.initialize().await?;

    let accounts: Arc<dyn AccountService> = account.clone();
    let identities: Arc<dyn IdentityResolver> = account;
    let storage_quotas: Arc<dyn StorageQuotaService> = Arc::new(PostgresStorageQuotaService::new(database.clone()));
    let preference_repository: Arc<dyn PreferenceRepository> = Arc::new(PostgresPreferenceRepository::new(database.clone()));
    let postgres = Arc::new(PostgresSyncRepository::new(database.clone()));
    let repository: Arc<dyn SyncRepository> = postgres.clone();
    let sync: Arc<dyn SyncService> = Arc::new(SyncApplication::new(repository));
    let preferences: Arc<dyn PreferenceService> = Arc::new(PreferenceApplication::new(preference_repository));
    let asset_repository: Arc<dyn AssetRepository> = postgres.clone();
    let coordinator: Arc<dyn MaintenanceCoordinator> = postgres;
    let asset_service: Arc<dyn AssetService> = Arc::new(AssetApplication::new(asset_repository, assets.clone()));
    let garbage_collector = Arc::new(GarbageCollector::new(coordinator, assets.clone()));
    let component_readiness: Arc<dyn server_http::ReadinessProbe> = Arc::new(ApplicationReadiness { database: database.clone(), assets: assets.clone() });
    let readiness: Arc<dyn server_http::ReadinessProbe> = Arc::new(server_http::CachedReadiness::new(component_readiness, READINESS_CACHE_TTL));
    let administration = server_http::AdministrationServices::with_readiness(storage_quotas, garbage_collector, readiness);
    let account_routes = server_account_http::router_with_public_registration(accounts, identities.clone(), client_address_source, public_registration);
    let sync_routes = server_http::router_with_operational_limits(identities, sync, preferences, asset_service, administration, transfer_limits, notification_config);
    Ok(request_limits.apply(account_routes.merge(sync_routes)))
}

struct ApplicationReadiness {
    database: PostgresDatabase,
    assets: Arc<dyn AssetStore>,
}

#[axum::async_trait]
impl server_http::ReadinessProbe for ApplicationReadiness {
    async fn check(&self) -> Result<(), String> {
        tokio::time::timeout(READINESS_COMPONENT_TIMEOUT, server_database::check_ready(&self.database)).await.map_err(|_| "database readiness timed out".to_string())?.map_err(|error| format!("database: {error}"))?;
        tokio::time::timeout(READINESS_COMPONENT_TIMEOUT, self.assets.check_ready()).await.map_err(|_| "asset storage readiness timed out".to_string())?.map_err(|error| format!("asset storage: {error}"))
    }
}
