pub mod transport;

use axum::Router;
use server_asset_store::FsAssetStore;
use server_core::{AssetApplication, AssetRepository, AssetService, AssetStore, AuthService, GarbageCollector, MaintenanceCoordinator, StorageQuotaService, SyncApplication, SyncRepository, SyncService};
use server_postgres::{PostgresAuthService, PostgresDatabase, PostgresStorageQuotaService, PostgresSyncRepository};
use std::path::PathBuf;
use std::sync::Arc;
use sync_common::api::opds::{CatalogRegistry, CATALOG_REGISTRY_SCHEMA_VERSION};

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
    app_with_catalog_registry(database, client_address_source, transfer_limits, bundled_opds_catalog_registry().expect("bundled OPDS catalog registry is valid"))
}

pub fn app_with_catalog_registry(database: PostgresDatabase, client_address_source: server_http::ClientAddressSource, transfer_limits: server_http::TransferLimits, opds_catalogs: CatalogRegistry) -> Router {
    app_with_paths_client_address_source_and_catalogs(database, PathBuf::from("./books"), PathBuf::from("./thumbnails"), client_address_source, transfer_limits, opds_catalogs)
}

pub fn load_opds_catalog_registry(path: Option<&std::path::Path>) -> Result<CatalogRegistry, Box<dyn std::error::Error>> {
    let bytes = match path {
        Some(path) => std::fs::read(path)?,
        None => include_bytes!("../catalogs/opds.json").to_vec(),
    };
    let registry: CatalogRegistry = serde_json::from_slice(&bytes)?;
    validate_opds_catalog_registry(&registry)?;
    Ok(registry)
}

fn bundled_opds_catalog_registry() -> Result<CatalogRegistry, Box<dyn std::error::Error>> {
    load_opds_catalog_registry(None)
}

fn validate_opds_catalog_registry(registry: &CatalogRegistry) -> Result<(), Box<dyn std::error::Error>> {
    if registry.schema_version != CATALOG_REGISTRY_SCHEMA_VERSION {
        return Err(format!("unsupported OPDS catalog registry schema {}", registry.schema_version).into());
    }
    let mut ids = std::collections::HashSet::new();
    for catalog in &registry.catalogs {
        if catalog.id.is_empty() || catalog.id.len() > 80 || !catalog.id.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-') || !ids.insert(catalog.id.as_str()) {
            return Err(format!("invalid or duplicate OPDS catalog id {:?}", catalog.id).into());
        }
        if catalog.title.trim().is_empty() || catalog.title.len() > 160 || catalog.description.trim().is_empty() || catalog.description.len() > 1000 || catalog.languages.is_empty() {
            return Err(format!("OPDS catalog {} has invalid descriptive metadata", catalog.id).into());
        }
        let url = url::Url::parse(&catalog.url)?;
        if url.scheme() != "https" || url.host_str().is_none() || !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err(format!("OPDS catalog {} must use an absolute credential-free HTTPS URL", catalog.id).into());
        }
    }
    Ok(())
}

fn app_with_paths_and_client_address_source(database: PostgresDatabase, books: PathBuf, thumbnails: PathBuf, client_address_source: server_http::ClientAddressSource, transfer_limits: server_http::TransferLimits) -> Router {
    app_with_paths_client_address_source_and_catalogs(database, books, thumbnails, client_address_source, transfer_limits, bundled_opds_catalog_registry().expect("bundled OPDS catalog registry is valid"))
}

fn app_with_paths_client_address_source_and_catalogs(database: PostgresDatabase, books: PathBuf, thumbnails: PathBuf, client_address_source: server_http::ClientAddressSource, transfer_limits: server_http::TransferLimits, opds_catalogs: CatalogRegistry) -> Router {
    // Creation is also repeated asynchronously by individual writes, so a
    // startup filesystem failure is surfaced by the first affected operation.
    let _ = std::fs::create_dir_all(&books);
    let _ = std::fs::create_dir_all(&thumbnails);

    let auth: Arc<dyn AuthService> = Arc::new(PostgresAuthService::new(database.clone()));
    let storage_quotas: Arc<dyn StorageQuotaService> = Arc::new(PostgresStorageQuotaService::new(database.clone()));
    let postgres = Arc::new(PostgresSyncRepository::new(database));
    let repository: Arc<dyn SyncRepository> = postgres.clone();
    let sync: Arc<dyn SyncService> = Arc::new(SyncApplication::new(repository));
    let asset_repository: Arc<dyn AssetRepository> = postgres.clone();
    let coordinator: Arc<dyn MaintenanceCoordinator> = postgres;
    let assets: Arc<dyn AssetStore> = Arc::new(FsAssetStore::new(books, thumbnails));
    let asset_service: Arc<dyn AssetService> = Arc::new(AssetApplication::new(asset_repository, assets.clone()));
    let garbage_collector = Arc::new(GarbageCollector::new(coordinator, assets.clone()));
    server_http::router_with_catalog_registry(auth, sync, asset_service, storage_quotas, garbage_collector, client_address_source, transfer_limits, opds_catalogs)
}
