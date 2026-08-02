use crate::PostgresDatabase;
use async_trait::async_trait;
use server_core::{SetStorageQuota, StorageQuotaError, StorageQuotaService, StorageQuotaStatus};
use sqlx::{PgPool, Row};

const MAX_CHANGE_ID_BYTES: usize = 128;
const MAX_ACTOR_BYTES: usize = 128;
const MAX_REASON_BYTES: usize = 500;

fn internal(error: impl std::fmt::Display) -> StorageQuotaError {
    StorageQuotaError::Internal(error.to_string())
}

fn required_bounded<'a>(name: &str, value: &'a str, max_bytes: usize) -> Result<&'a str, StorageQuotaError> {
    let value = value.trim();
    if value.is_empty() || value.len() > max_bytes {
        return Err(StorageQuotaError::InvalidInput(format!("{name} must contain 1-{max_bytes} bytes")));
    }
    Ok(value)
}

fn as_u64(value: i64, name: &str) -> Result<u64, StorageQuotaError> {
    u64::try_from(value).map_err(|_| internal(format!("stored {name} is negative")))
}

#[derive(Clone)]
pub struct PostgresStorageQuotaService {
    pool: PgPool,
}

impl PostgresStorageQuotaService {
    pub fn new(database: PostgresDatabase) -> Self {
        Self { pool: database.pool().clone() }
    }
}

#[async_trait]
impl StorageQuotaService for PostgresStorageQuotaService {
    async fn set_quota(&self, change: SetStorageQuota) -> Result<StorageQuotaStatus, StorageQuotaError> {
        let change_id = required_bounded("change_id", &change.change_id, MAX_CHANGE_ID_BYTES)?;
        let user_id = required_bounded("user_id", &change.user_id, 128)?;
        let actor = required_bounded("actor", &change.actor, MAX_ACTOR_BYTES)?;
        let reason = required_bounded("reason", &change.reason, MAX_REASON_BYTES)?;
        let quota_bytes = i64::try_from(change.quota_bytes).map_err(|_| StorageQuotaError::InvalidInput("quota_bytes exceeds PostgreSQL BIGINT".to_string()))?;

        let mut transaction = self.pool.begin().await.map_err(internal)?;
        // Serializes retries across server processes before any account lock is
        // taken, so one change_id always describes exactly one mutation.
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))").bind(format!("bokheim:quota-change:{change_id}")).execute(&mut *transaction).await.map_err(internal)?;

        let previous = sqlx::query(
            "SELECT user_id, actor, new_quota_bytes, reason
             FROM storage_quota_change
             WHERE change_id=$1",
        )
        .bind(change_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(internal)?;
        if let Some(previous) = previous {
            let identical = previous.get::<String, _>("user_id") == user_id && previous.get::<String, _>("actor") == actor && previous.get::<i64, _>("new_quota_bytes") == quota_bytes && previous.get::<String, _>("reason") == reason;
            if !identical {
                return Err(StorageQuotaError::IdempotencyConflict);
            }
            let row = sqlx::query("SELECT quota_bytes, used_bytes, reserved_bytes FROM user_storage_account WHERE user_id=$1").bind(user_id).fetch_one(&mut *transaction).await.map_err(internal)?;
            transaction.commit().await.map_err(internal)?;
            return Ok(StorageQuotaStatus { quota_bytes: as_u64(row.get("quota_bytes"), "quota_bytes")?, used_bytes: as_u64(row.get("used_bytes"), "used_bytes")?, reserved_bytes: as_u64(row.get("reserved_bytes"), "reserved_bytes")? });
        }

        sqlx::query(
            "INSERT INTO user_storage_account(user_id)
             SELECT id FROM users WHERE id=$1
             ON CONFLICT (user_id) DO NOTHING",
        )
        .bind(user_id)
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;
        let account = sqlx::query(
            "SELECT quota_bytes, used_bytes, reserved_bytes
             FROM user_storage_account
             WHERE user_id=$1
             FOR UPDATE",
        )
        .bind(user_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(internal)?
        .ok_or(StorageQuotaError::UserNotFound)?;
        let previous_quota: i64 = account.get("quota_bytes");
        let used: i64 = account.get("used_bytes");
        let reserved: i64 = account.get("reserved_bytes");

        sqlx::query("UPDATE user_storage_account SET quota_bytes=$2 WHERE user_id=$1").bind(user_id).bind(quota_bytes).execute(&mut *transaction).await.map_err(internal)?;
        sqlx::query(
            "INSERT INTO storage_quota_change(
                 change_id, user_id, actor, previous_quota_bytes,
                 new_quota_bytes, reason
             ) VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(change_id)
        .bind(user_id)
        .bind(actor)
        .bind(previous_quota)
        .bind(quota_bytes)
        .bind(reason)
        .execute(&mut *transaction)
        .await
        .map_err(internal)?;
        transaction.commit().await.map_err(internal)?;

        Ok(StorageQuotaStatus { quota_bytes: change.quota_bytes, used_bytes: as_u64(used, "used_bytes")?, reserved_bytes: as_u64(reserved, "reserved_bytes")? })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn isolated_pool() -> Option<PgPool> {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").ok()?;
        let root = PgPoolOptions::new().max_connections(1).connect(&database_url).await.unwrap();
        let schema = format!("quota_admin_{}", uuid::Uuid::new_v4().simple());
        sqlx::query(&format!("CREATE SCHEMA {schema}")).execute(&root).await.unwrap();
        drop(root);
        let separator = if database_url.contains('?') { '&' } else { '?' };
        let scoped_url = format!("{database_url}{separator}options=-csearch_path%3D{schema}");
        Some(PgPoolOptions::new().max_connections(4).connect(&scoped_url).await.unwrap())
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn downgrade_can_leave_account_over_quota_and_is_audited_idempotently() {
        let Some(pool) = isolated_pool().await else { return };
        crate::migrate(&PostgresDatabase::from_pool(pool.clone())).await.unwrap();
        let user_id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO users(id, username) VALUES ($1, $2)").bind(&user_id).bind(format!("quota-{}", uuid::Uuid::new_v4().simple())).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO user_storage_account(user_id, quota_bytes, used_bytes) VALUES ($1, 100, 80)").bind(&user_id).execute(&pool).await.unwrap();
        let service = PostgresStorageQuotaService { pool: pool.clone() };
        let change = SetStorageQuota { change_id: "downgrade-1".to_string(), user_id: user_id.clone(), actor: "billing".to_string(), quota_bytes: 50, reason: "subscription downgraded".to_string() };

        let status = service.set_quota(change.clone()).await.unwrap();
        assert!(status.over_quota());
        assert_eq!(status, service.set_quota(change.clone()).await.unwrap());
        let audit_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM storage_quota_change WHERE change_id=$1").bind(&change.change_id).fetch_one(&pool).await.unwrap();
        assert_eq!(audit_count, 1);

        let conflicting = SetStorageQuota { quota_bytes: 60, ..change };
        assert!(matches!(service.set_quota(conflicting).await, Err(StorageQuotaError::IdempotencyConflict)));
    }
}
