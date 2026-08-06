//! PostgreSQL adapter for synchronization, quota accounting, and cross-process
//! maintenance coordination. Account persistence lives in
//! `server-account-postgres`; schema migration lives in `server-database`.

mod assets;
mod preferences;
mod quota;
mod sync;
mod sync_store;

pub use preferences::PostgresPreferenceRepository;
pub use quota::PostgresStorageQuotaService;
pub use sync::PostgresSyncRepository;

pub use server_database::{connect, migrate, PostgresDatabase};
