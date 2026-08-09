//! PostgreSQL adapter for synchronization, asset storage, and cross-process
//! maintenance coordination. Account persistence lives in
//! Account persistence is implemented by `server-account`; schema migration
//! lives in `server-database`.

mod applications;
mod assets;
mod sync;
mod sync_store;
#[cfg(test)]
mod test_support;

pub use applications::AssetApplication;
pub use sync::{GarbageCollector, PostgresSyncRepository};

pub use server_database::{connect, migrate, PostgresDatabase};
