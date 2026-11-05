//! Shared PostgreSQL capability plus synchronization and asset persistence.
//! Account persistence remains implemented by `server-account`.

mod applications;
mod assets;
mod cleanup;
mod cloud_storage;
mod database;
mod sync;
mod sync_store;

pub use applications::{AssetApplication, BookStorageUsage, LibraryStorageUsage, StorageUsage};
pub use cleanup::CleanupResult;
pub async fn configure_user_storage_quota(database: &PostgresDatabase, user_id: &str, quota_bytes: i64) -> Result<i64, sqlx::Error> {
    assets::configure_user_storage_quota(database, user_id, quota_bytes).await
}
pub use database::{DatabasePoolConfig, PostgresDatabase, check_ready, connect, connect_configured, migrate, rotate_sync_generation, verify_schema};
pub use sync::{PostgresSyncRepository, SyncError};
