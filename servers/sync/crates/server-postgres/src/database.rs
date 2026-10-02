//! Shared PostgreSQL connection and initial schema setup.

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

const SCHEMA_VERSION: i32 = 4;

/// Applies the initial schema to an empty database. Files are
/// applied in filename order -- `01_core.sql` before the domain files that
/// reference it.
pub async fn migrate(database: &PostgresDatabase) -> Result<(), sqlx::Error> {
    const SCHEMA_FILES: &[&str] = &[include_str!("../schema/01_core.sql"), include_str!("../schema/02_accounts.sql"), include_str!("../schema/03_sync.sql"), include_str!("../schema/04_assets.sql")];
    for file in SCHEMA_FILES {
        sqlx::raw_sql(file).execute(database.pool()).await?;
    }
    Ok(())
}

/// Adopt the deployed pre-release baseline and upgrade it to the current schema
/// in one transaction, preserving data.
/// Startup remains read-only; only the explicit deployment command upgrades it.
pub async fn upgrade_existing(database: &PostgresDatabase) -> Result<(), sqlx::Error> {
    let mut transaction = database.pool().begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtext('bokheim-schema-upgrade'))").execute(&mut *transaction).await?;
    let versioned: bool = sqlx::query_scalar("SELECT to_regclass('bokheim_schema_version') IS NOT NULL").fetch_one(&mut *transaction).await?;
    if !versioned {
        sqlx::raw_sql(include_str!("../schema/migrations/adopt_release_baseline.sql")).execute(&mut *transaction).await?;
    }
    let version: i32 = sqlx::query_scalar("SELECT version FROM bokheim_schema_version").fetch_one(&mut *transaction).await?;
    if !(1..=SCHEMA_VERSION).contains(&version) {
        return Err(sqlx::Error::Protocol(format!("unsupported sync schema version {version}; expected {SCHEMA_VERSION}")));
    }
    if version == 1 {
        sqlx::raw_sql(include_str!("../schema/migrations/book_owner.sql")).execute(&mut *transaction).await?;
    }
    if version <= 2 {
        sqlx::raw_sql(include_str!("../schema/migrations/shrink_identity.sql")).execute(&mut *transaction).await?;
    }
    if version <= 3 {
        sqlx::raw_sql(include_str!("../schema/migrations/merge_sync_state.sql")).execute(&mut *transaction).await?;
    }
    transaction.commit().await?;
    Ok(())
}

/// Reject databases created with earlier development schemas before activating a release.
pub async fn verify_schema(database: &PostgresDatabase) -> Result<(), sqlx::Error> {
    let version: i32 = sqlx::query_scalar("SELECT version FROM bokheim_schema_version").fetch_one(database.pool()).await?;
    if version != SCHEMA_VERSION {
        return Err(sqlx::Error::Protocol(format!("unsupported sync schema version {version}; expected {SCHEMA_VERSION}")));
    }
    Ok(())
}

pub async fn check_ready(database: &PostgresDatabase) -> Result<(), sqlx::Error> {
    sqlx::query_scalar::<_, i32>("SELECT 1").fetch_one(database.pool()).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use crate::test_support::isolated_pool;
    use super::DatabasePoolConfig;
    use std::collections::HashMap;

    fn config(values: &[(&str, &str)]) -> Result<DatabasePoolConfig, String> {
        let values: HashMap<_, _> = values.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect();
        DatabasePoolConfig::from_values(|name| values.get(name).cloned())
    }

    /// Schema 1 stored `present` and left `content_hash` NULL on removals and
    /// purges. The upgrade names each value's book and drops `present`.
    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn schema_1_rows_gain_their_book_and_lose_present() {
        let Some(url) = crate::test_support::isolated_schema_url("book_owner_upgrade").await else { return };
        let pool = sqlx::PgPool::connect(&url).await.unwrap();
        sqlx::raw_sql(&include_str!("../schema/01_core.sql").replace("version = 4", "version = 2").replace("VALUES (4)", "VALUES (2)")).execute(&pool).await.unwrap();
        sqlx::raw_sql(include_str!("../schema/fixtures/sync_v2.sql")).execute(&pool).await.unwrap();
        sqlx::raw_sql(
            "ALTER TABLE sync_state ADD COLUMN present boolean;
             ALTER TABLE sync_reading_state ADD COLUMN present boolean;
             ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check;
             UPDATE bokheim_schema_version SET version = 1;
             ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK (version = 1);
             INSERT INTO users(id, email) VALUES('u', 'u@example.com');
             INSERT INTO libraries(id, user_id, name) VALUES('l', 'u', 'Books');
             INSERT INTO sync_state(library_id,kind,entity_key,entity_subkey,value,present,content_hash,changed_at,replica_id,replica_seq,version_rank,event_id) VALUES
                 ('l','book_lifecycle','aa','',''::bytea,FALSE,NULL,1,'r',1,2,'e1'),
                 ('l','placement','aa','dir',''::bytea,FALSE,NULL,1,'r',2,0,'e2'),
                 ('l','annotation','note','',''::bytea,FALSE,'bb',1,'r',3,1,'e3'),
                 ('l','directory_name','dir','',''::bytea,NULL,NULL,1,'r',4,0,'e4');
             INSERT INTO sync_reading_state(library_id,kind,entity_key,entity_subkey,value,present,content_hash,changed_at,replica_id,replica_seq,version_rank,event_id) VALUES
                 ('l','reading_position','aa','',''::bytea,NULL,NULL,1,'r',5,0,'e5');",
        )
        .execute(&pool)
        .await
        .unwrap();

        super::upgrade_existing(&super::PostgresDatabase::from_pool(pool.clone())).await.unwrap();

        let books: Vec<(String, Option<String>)> = sqlx::query_as("SELECT kind, content_hash FROM sync_state ORDER BY 1").fetch_all(&pool).await.unwrap();
        let expected = [("annotation", Some("bb")), ("book_lifecycle", Some("aa")), ("directory_name", None), ("placement", Some("aa")), ("reading_position", Some("aa"))];
        assert_eq!(books, expected.map(|(kind, book)| (kind.to_owned(), book.map(str::to_owned))));
        let present: i64 = sqlx::query_scalar("SELECT count(*) FROM information_schema.columns WHERE column_name='present' AND table_name IN ('sync_state','sync_reading_state') AND table_schema=current_schema()").fetch_one(&pool).await.unwrap();
        assert_eq!(present, 0);
        assert_eq!(sqlx::query_scalar::<_, i32>("SELECT version FROM bokheim_schema_version").fetch_one(&pool).await.unwrap(), super::SCHEMA_VERSION);
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn schema_2_and_3_merge_preserves_values_versions_and_delivery_heads() {
        for starting_version in [2, 3] {
            let url = crate::test_support::isolated_schema_url("sync_merge_upgrade").await.unwrap();
            let pool = sqlx::PgPool::connect(&url).await.unwrap();
            sqlx::raw_sql(&include_str!("../schema/01_core.sql").replace("version = 4", "version = 2").replace("VALUES (4)", "VALUES (2)")).execute(&pool).await.unwrap();
            sqlx::raw_sql(include_str!("../schema/fixtures/sync_v2.sql")).execute(&pool).await.unwrap();
            sqlx::raw_sql(
                "INSERT INTO users(id) VALUES('u');
                 INSERT INTO libraries(id,user_id,name) VALUES('l','u','Books');
                 INSERT INTO sync_state(library_id,kind,entity_key,value,content_hash,changed_at,replica_id,replica_seq,version_rank,event_id) VALUES
                   ('l','directory_name','folder',decode('0102','hex'),NULL,10,'00000000-0000-0000-0000-000000000001',1,0,'e1'),
                   ('l','book_lifecycle','book',decode('0304','hex'),'book',20,'00000000-0000-0000-0000-000000000001',2,0,'e2');
                 INSERT INTO sync_reading_state(library_id,kind,entity_key,value,content_hash,changed_at,replica_id,replica_seq,version_rank,event_id) VALUES
                   ('l','reading_position','book',decode('0506','hex'),'book',30,'00000000-0000-0000-0000-000000000002',1,0,'e3');"
            ).execute(&pool).await.unwrap();
            if starting_version == 3 {
                sqlx::raw_sql(include_str!("../schema/migrations/shrink_identity.sql")).execute(&pool).await.unwrap();
            }
            let snapshot = "SELECT jsonb_agg(row ORDER BY kind)::text FROM (
                SELECT kind,entity_key,entity_subkey,value,content_hash,changed_at,replica_id,replica_seq,version_rank,server_seq FROM sync_state
                UNION ALL SELECT kind,entity_key,entity_subkey,value,content_hash,changed_at,replica_id,replica_seq,version_rank,server_seq FROM sync_reading_state
            ) row";
            let before: String = sqlx::query_scalar(snapshot).fetch_one(&pool).await.unwrap();
            let head: i64 = sqlx::query_scalar("SELECT current_revision FROM sync_library_revision WHERE library_id='l'").fetch_one(&pool).await.unwrap();
            let database = super::PostgresDatabase::from_pool(pool.clone());
            super::upgrade_existing(&database).await.unwrap();
            super::verify_schema(&database).await.unwrap();
            super::upgrade_existing(&database).await.unwrap();
            let after: String = sqlx::query_scalar("SELECT jsonb_agg(row ORDER BY kind)::text FROM (SELECT kind,entity_key,entity_subkey,value,content_hash,changed_at,replica_id,replica_seq,version_rank,server_seq FROM sync_state) row").fetch_one(&pool).await.unwrap();
            assert_eq!(after, before, "upgrade from schema {starting_version} changed retained state");
            assert_eq!(sqlx::query_scalar::<_, i64>("SELECT current_revision FROM sync_library_revision WHERE library_id='l'").fetch_one(&pool).await.unwrap(), head);
            assert_eq!(sqlx::query_scalar::<_, i64>("SELECT last_value FROM sync_server_sequence").fetch_one(&pool).await.unwrap(), head, "migration must not allocate revisions");
            let obsolete: i64 = sqlx::query_scalar("SELECT count(*) FROM information_schema.columns WHERE table_schema=current_schema() AND ((table_name='sync_library_revision' AND column_name IN ('reading_revision','other_revision')) OR (table_name='sync_state' AND column_name='event_id') OR table_name='sync_reading_state')").fetch_one(&pool).await.unwrap();
            assert_eq!(obsolete, 0);
            sqlx::query("UPDATE sync_state SET changed_at=40,replica_seq=2,server_seq=DEFAULT WHERE library_id='l' AND kind='reading_position'").execute(&pool).await.unwrap();
            assert!(sqlx::query_scalar::<_, i64>("SELECT current_revision FROM sync_library_revision WHERE library_id='l'").fetch_one(&pool).await.unwrap() > head, "merged reading writes must advance the committed head");
        }
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
