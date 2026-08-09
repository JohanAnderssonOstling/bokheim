//! Shared PostgreSQL capability plus synchronization and asset persistence.
//! Account persistence remains implemented by `server-account`.

mod applications;
mod assets;
mod database;
mod sync;
mod sync_store;
#[cfg(test)]
mod test_support;

pub use applications::AssetApplication;
pub use database::{check_ready, connect, connect_configured, migrate, DatabasePoolConfig, PostgresDatabase};
pub use sync::{PostgresSyncRepository, SyncError};
