//! PostgreSQL adapter for synchronization, authentication, migrations, and
//! cross-process maintenance coordination.

mod assets;
mod auth;
mod quota;
mod sync;
mod sync_store;

pub use auth::PostgresAuthService;
pub use quota::PostgresStorageQuotaService;
pub use sync::PostgresSyncRepository;

use sqlx::PgPool;

/// Opaque database capability exposed to the composition root. SQLx pool
/// access remains private to this adapter crate.
#[derive(Clone)]
pub struct PostgresDatabase {
    pool: PgPool,
}

impl PostgresDatabase {
    pub(crate) fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }
}

pub async fn connect(database_url: &str) -> Result<PostgresDatabase, sqlx::Error> {
    PgPool::connect(database_url).await.map(PostgresDatabase::from_pool)
}

pub async fn migrate(database: &PostgresDatabase) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(database.pool()).await
}
