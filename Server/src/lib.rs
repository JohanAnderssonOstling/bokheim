pub mod storage_config;
pub mod transport;

use axum::Router;
use server_account::{ClientAddressSource, PostgresAccountService};
use server_asset_store::FsAssetStore;
use server_core::AssetError;
use server_database::PostgresDatabase;
use server_postgres::{AssetApplication, PostgresSyncRepository};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const READINESS_CACHE_TTL: Duration = Duration::from_secs(5);

pub struct AppConfig {
    pub public_registration: bool,
    pub books: PathBuf,
    pub thumbnails: PathBuf,
    pub client_address_source: ClientAddressSource,
    pub transfer_limits: server_http::TransferLimits,
    pub request_limits: server_http::RequestLimits,
    pub upload_limits: server_asset_store::UploadLimits,
    pub notification_config: server_http::NotificationConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            public_registration: false,
            books: PathBuf::from("./books"),
            thumbnails: PathBuf::from("./thumbnails"),
            client_address_source: ClientAddressSource::DirectPeer,
            transfer_limits: server_http::TransferLimits::default(),
            request_limits: server_http::RequestLimits::default(),
            upload_limits: server_asset_store::UploadLimits::default(),
            notification_config: server_http::NotificationConfig::default(),
        }
    }
}

pub async fn app(database: PostgresDatabase, account: Arc<PostgresAccountService>, config: AppConfig) -> Result<Router, AssetError> {
    let AppConfig { public_registration, books, thumbnails, client_address_source, transfer_limits, request_limits, upload_limits, notification_config } = config;

    let assets = Arc::new(FsAssetStore::with_upload_limits(books, thumbnails, upload_limits));
    assets.initialize().await?;

    let postgres = Arc::new(PostgresSyncRepository::new(database.clone()));
    let sync = postgres.clone();
    let asset_service = Arc::new(AssetApplication::new(postgres, assets.clone()));
    let readiness = Arc::new(server_http::Readiness::new(database.clone(), assets.clone(), READINESS_CACHE_TTL));
    let account_routes = server_account::http::router_with_public_registration(account.clone(), client_address_source, public_registration);
    let sync_routes = server_http::router_with_operational_limits(account, sync, asset_service, readiness, transfer_limits, notification_config);
    Ok(request_limits.apply(account_routes.merge(sync_routes)))
}
