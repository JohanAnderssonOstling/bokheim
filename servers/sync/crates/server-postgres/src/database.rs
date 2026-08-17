//! Shared PostgreSQL connection capability and schema migration boundary.

use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DatabasePoolConfig {
    pub max_connections: u32,
    pub min_connections: u32,
    pub acquire_timeout: Duration,
    pub idle_timeout: Duration,
}

impl Default for DatabasePoolConfig {
    fn default() -> Self {
        Self { max_connections: 32, min_connections: 2, acquire_timeout: Duration::from_secs(5), idle_timeout: Duration::from_secs(10 * 60) }
    }
}

impl DatabasePoolConfig {
    pub fn from_env() -> Result<Self, String> {
        Self::from_values(|name| std::env::var(name).ok())
    }

    fn from_values(mut value: impl FnMut(&str) -> Option<String>) -> Result<Self, String> {
        let defaults = Self::default();
        let max_connections = parse_bounded_u32(value("BOKHEIM_DATABASE_MAX_CONNECTIONS"), defaults.max_connections, 2, 256, "BOKHEIM_DATABASE_MAX_CONNECTIONS")?;
        let min_connections = parse_bounded_u32(value("BOKHEIM_DATABASE_MIN_CONNECTIONS"), defaults.min_connections, 0, max_connections, "BOKHEIM_DATABASE_MIN_CONNECTIONS")?;
        let acquire_seconds = parse_bounded_u64(value("BOKHEIM_DATABASE_ACQUIRE_TIMEOUT_SECONDS"), defaults.acquire_timeout.as_secs(), 1, 60, "BOKHEIM_DATABASE_ACQUIRE_TIMEOUT_SECONDS")?;
        let idle_seconds = parse_bounded_u64(value("BOKHEIM_DATABASE_IDLE_TIMEOUT_SECONDS"), defaults.idle_timeout.as_secs(), 30, 24 * 60 * 60, "BOKHEIM_DATABASE_IDLE_TIMEOUT_SECONDS")?;
        Ok(Self { max_connections, min_connections, acquire_timeout: Duration::from_secs(acquire_seconds), idle_timeout: Duration::from_secs(idle_seconds) })
    }
}

fn parse_bounded_u32(raw: Option<String>, default: u32, minimum: u32, maximum: u32, name: &str) -> Result<u32, String> {
    let value = raw.map(|raw| raw.parse::<u32>().map_err(|_| format!("{name} must be an integer"))).transpose()?.unwrap_or(default);
    (minimum..=maximum).contains(&value).then_some(value).ok_or_else(|| format!("{name} must be between {minimum} and {maximum}"))
}

fn parse_bounded_u64(raw: Option<String>, default: u64, minimum: u64, maximum: u64, name: &str) -> Result<u64, String> {
    let value = raw.map(|raw| raw.parse::<u64>().map_err(|_| format!("{name} must be an integer"))).transpose()?.unwrap_or(default);
    (minimum..=maximum).contains(&value).then_some(value).ok_or_else(|| format!("{name} must be between {minimum} and {maximum}"))
}

#[derive(Clone)]
pub struct PostgresDatabase {
    pool: PgPool,
}

impl PostgresDatabase {
    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

pub async fn connect(database_url: &str) -> Result<PostgresDatabase, sqlx::Error> {
    PgPool::connect(database_url).await.map(PostgresDatabase::from_pool)
}

pub async fn connect_configured(database_url: &str, config: DatabasePoolConfig) -> Result<PostgresDatabase, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(config.acquire_timeout)
        .idle_timeout(Some(config.idle_timeout))
        .connect(database_url)
        .await
        .map(PostgresDatabase::from_pool)
}

/// Applies the schema in `schema/*.sql` (embedded at compile time) to an
/// empty database. There is no migration history: `servers/sync/deploy/install-server.sh`
/// drops and recreates the database on every deploy, so this only ever runs
/// against a fresh, empty schema. Files are applied in filename order --
/// `01_core.sql` before the domain files that reference it.
pub async fn migrate(database: &PostgresDatabase) -> Result<(), sqlx::Error> {
    const SCHEMA_FILES: &[&str] = &[include_str!("../schema/01_core.sql"), include_str!("../schema/02_accounts.sql"), include_str!("../schema/03_sync.sql"), include_str!("../schema/04_assets.sql"), include_str!("../schema/05_admin.sql")];
    for file in SCHEMA_FILES {
        sqlx::raw_sql(file).execute(database.pool()).await?;
    }
    Ok(())
}

/// Upgrades an existing production database without recreating or deleting
/// user data. The embedded SQL is transactional, serialized, and idempotent.
pub async fn upgrade_existing(database: &PostgresDatabase) -> Result<(), sqlx::Error> {
    sqlx::raw_sql(include_str!("../schema/upgrade_20260817.sql")).execute(database.pool()).await?;
    Ok(())
}

pub async fn check_ready(database: &PostgresDatabase) -> Result<(), sqlx::Error> {
    sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(database.pool()).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::DatabasePoolConfig;
    use std::collections::HashMap;

    fn config(values: &[(&str, &str)]) -> Result<DatabasePoolConfig, String> {
        let values: HashMap<_, _> = values.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect();
        DatabasePoolConfig::from_values(|name| values.get(name).cloned())
    }

    #[test]
    fn database_pool_defaults_and_bounds_are_operationally_safe() {
        assert_eq!(config(&[]).unwrap(), DatabasePoolConfig::default());
        let configured = config(&[("BOKHEIM_DATABASE_MAX_CONNECTIONS", "48"), ("BOKHEIM_DATABASE_MIN_CONNECTIONS", "4"), ("BOKHEIM_DATABASE_ACQUIRE_TIMEOUT_SECONDS", "8")]).unwrap();
        assert_eq!(configured.max_connections, 48);
        assert_eq!(configured.min_connections, 4);
        assert_eq!(configured.acquire_timeout.as_secs(), 8);
        assert!(config(&[("BOKHEIM_DATABASE_MAX_CONNECTIONS", "1")]).is_err());
        assert!(config(&[("BOKHEIM_DATABASE_MAX_CONNECTIONS", "8"), ("BOKHEIM_DATABASE_MIN_CONNECTIONS", "9")]).is_err());
        assert!(config(&[("BOKHEIM_DATABASE_ACQUIRE_TIMEOUT_SECONDS", "0")]).is_err());
    }
}
