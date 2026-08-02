pub mod transport;

use axum::Router;
use server_asset_store::FsAssetStore;
use server_core::{AssetApplication, AssetRepository, AssetService, AssetStore, AuthService, GarbageCollector, MaintenanceCoordinator, SyncApplication, SyncRepository, SyncService};
use server_postgres::{PostgresAuthService, PostgresDatabase, PostgresSyncRepository};
use std::path::PathBuf;
use std::sync::Arc;

pub fn app(database: PostgresDatabase) -> Router {
    app_with_client_address_source(database, server_http::ClientAddressSource::DirectPeer)
}

pub fn app_with_paths(database: PostgresDatabase, books: PathBuf, thumbnails: PathBuf) -> Router {
    app_with_paths_and_client_address_source(database, books, thumbnails, server_http::ClientAddressSource::DirectPeer, server_http::TransferLimits::default())
}

pub fn app_with_client_address_source(database: PostgresDatabase, client_address_source: server_http::ClientAddressSource) -> Router {
    app_with_paths_and_client_address_source(database, PathBuf::from("./books"), PathBuf::from("./thumbnails"), client_address_source, server_http::TransferLimits::default())
}

pub fn app_with_client_address_source_and_transfer_limits(database: PostgresDatabase, client_address_source: server_http::ClientAddressSource, transfer_limits: server_http::TransferLimits) -> Router {
    app_with_paths_and_client_address_source(database, PathBuf::from("./books"), PathBuf::from("./thumbnails"), client_address_source, transfer_limits)
}

fn app_with_paths_and_client_address_source(
    database: PostgresDatabase,
    books: PathBuf,
    thumbnails: PathBuf,
    client_address_source: server_http::ClientAddressSource,
    transfer_limits: server_http::TransferLimits,
) -> Router {
    // Creation is also repeated asynchronously by individual writes, so a
    // startup filesystem failure is surfaced by the first affected operation.
    let _ = std::fs::create_dir_all(&books);
    let _ = std::fs::create_dir_all(&thumbnails);

    let auth: Arc<dyn AuthService> = Arc::new(PostgresAuthService::new(database.clone()));
    let postgres = Arc::new(PostgresSyncRepository::new(database));
    let repository: Arc<dyn SyncRepository> = postgres.clone();
    let sync: Arc<dyn SyncService> = Arc::new(SyncApplication::new(repository));
    let asset_repository: Arc<dyn AssetRepository> = postgres.clone();
    let coordinator: Arc<dyn MaintenanceCoordinator> = postgres;
    let assets: Arc<dyn AssetStore> = Arc::new(FsAssetStore::new(books, thumbnails));
    let asset_service: Arc<dyn AssetService> = Arc::new(AssetApplication::new(asset_repository, assets.clone()));
    let garbage_collector = Arc::new(GarbageCollector::new(coordinator, assets.clone()));
    server_http::router_with_client_address_source_and_transfer_limits(auth, sync, asset_service, garbage_collector, client_address_source, transfer_limits)
}
