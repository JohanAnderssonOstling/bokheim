use chrono::{Duration, Utc};
use server_core::{AssetError, BookUploadAdmission, BookUploadReservation};
use sqlx::{PgPool, Postgres, Row, Transaction};
use sync_common::{ContentHash, LibraryId};

const UPLOAD_RESERVATION_TTL: Duration = Duration::minutes(30);

fn storage(error: impl std::fmt::Display) -> AssetError {
    AssetError::Storage(error.to_string())
}

fn size_i64(size: u64) -> Result<i64, AssetError> {
    i64::try_from(size).map_err(|_| AssetError::Storage("book size does not fit the quota ledger".to_string()))
}

async fn lock_library_and_require_hash(transaction: &mut Transaction<'_, Postgres>, user_id: &str, library_id: &LibraryId, hash: &ContentHash) -> Result<(), AssetError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))").bind(library_id.as_str()).execute(&mut **transaction).await.map_err(storage)?;
    let authorized = sqlx::query_scalar::<_, bool>(
        "WITH latest_per_occupant AS (
             SELECT DISTINCT ON (entity_id, data->>'dir_id', data->>'file_name')
                 data->>'dir_id' AS dir_id,
                 data->>'file_name' AS file_name,
                 event_type,
                 data->>'content_hash' AS content_hash,
                 hlc_pt, hlc_l, version_rank, client_id, client_seq, event_id
             FROM library_events
             WHERE library_id=$1 AND event_type IN ('book_dir_added', 'book_dir_removed')
             ORDER BY entity_id, data->>'dir_id', data->>'file_name',
                      hlc_pt DESC, hlc_l DESC, version_rank DESC,
                      client_id COLLATE \"C\" DESC, client_seq DESC, event_id COLLATE \"C\" DESC
         ), latest_per_path AS (
             SELECT DISTINCT ON (dir_id, file_name) content_hash
             FROM latest_per_occupant
             WHERE event_type='book_dir_added'
             ORDER BY dir_id, file_name,
                      hlc_pt DESC, hlc_l DESC, version_rank DESC,
                      client_id COLLATE \"C\" DESC, client_seq DESC, event_id COLLATE \"C\" DESC
         )
         SELECT EXISTS(
             SELECT 1
             FROM latest_per_path
             WHERE content_hash=$3
         ) AND EXISTS(
             SELECT 1 FROM libraries WHERE id=$1 AND user_id=$2
         )",
    )
    .bind(library_id.as_str())
    .bind(user_id)
    .bind(hash.as_str())
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage)?;
    if authorized {
        Ok(())
    } else {
        Err(AssetError::Forbidden)
    }
}

async fn lock_account(transaction: &mut Transaction<'_, Postgres>, user_id: &str) -> Result<(i64, i64, i64), AssetError> {
    sqlx::query("INSERT INTO user_storage_account(user_id) VALUES ($1) ON CONFLICT (user_id) DO NOTHING").bind(user_id).execute(&mut **transaction).await.map_err(storage)?;
    let row = sqlx::query("SELECT quota_bytes, used_bytes, reserved_bytes FROM user_storage_account WHERE user_id=$1 FOR UPDATE").bind(user_id).fetch_one(&mut **transaction).await.map_err(storage)?;
    Ok((row.get("quota_bytes"), row.get("used_bytes"), row.get("reserved_bytes")))
}

async fn expire_reservations(transaction: &mut Transaction<'_, Postgres>, user_id: &str) -> Result<i64, AssetError> {
    let released = sqlx::query_scalar::<_, i64>(
        "WITH expired AS (
             DELETE FROM book_upload_reservation
             WHERE user_id=$1 AND expires_at <= now()
             RETURNING size_bytes
         )
         SELECT COALESCE(SUM(size_bytes), 0)::BIGINT FROM expired",
    )
    .bind(user_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage)?;
    if released > 0 {
        sqlx::query("UPDATE user_storage_account SET reserved_bytes=reserved_bytes-$2 WHERE user_id=$1").bind(user_id).bind(released).execute(&mut **transaction).await.map_err(storage)?;
    }
    Ok(released)
}

fn ensure_capacity(limit: i64, used: i64, reserved: i64, requested: i64) -> Result<(), AssetError> {
    if i128::from(used) + i128::from(reserved) + i128::from(requested) <= i128::from(limit) {
        return Ok(());
    }
    Err(AssetError::QuotaExceeded { limit: limit.max(0) as u64, used: used.max(0) as u64, reserved: reserved.max(0) as u64, requested: requested.max(0) as u64 })
}

async fn insert_blob_object(transaction: &mut Transaction<'_, Postgres>, hash: &ContentHash, size_bytes: i64) -> Result<(), AssetError> {
    let inserted = sqlx::query_scalar::<_, String>(
        "INSERT INTO blob_object(content_hash, size_bytes) VALUES ($1, $2)
         ON CONFLICT (content_hash) DO UPDATE SET size_bytes=blob_object.size_bytes
         WHERE blob_object.size_bytes=EXCLUDED.size_bytes
         RETURNING content_hash",
    )
    .bind(hash.as_str())
    .bind(size_bytes)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage)?;
    if inserted.is_some() {
        Ok(())
    } else {
        Err(AssetError::Storage(format!("stored size for content hash {hash} is inconsistent")))
    }
}

pub(super) async fn claim_existing_book(pool: &PgPool, user_id: &str, library_id: &LibraryId, hash: &ContentHash, size_bytes: u64) -> Result<(), AssetError> {
    let size_bytes = size_i64(size_bytes)?;
    let mut transaction = pool.begin().await.map_err(storage)?;
    lock_library_and_require_hash(&mut transaction, user_id, library_id, hash).await?;
    let (limit, used, mut reserved) = lock_account(&mut transaction, user_id).await?;
    reserved -= expire_reservations(&mut transaction, user_id).await?;

    if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM user_blob_charge WHERE user_id=$1 AND content_hash=$2)").bind(user_id).bind(hash.as_str()).fetch_one(&mut *transaction).await.map_err(storage)? {
        transaction.commit().await.map_err(storage)?;
        return Ok(());
    }

    let active_reservation = sqlx::query_scalar::<_, Option<i64>>("DELETE FROM book_upload_reservation WHERE user_id=$1 AND content_hash=$2 RETURNING size_bytes")
        .bind(user_id)
        .bind(hash.as_str())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(storage)?
        .flatten();
    if let Some(released) = active_reservation {
        reserved -= released;
        sqlx::query("UPDATE user_storage_account SET reserved_bytes=reserved_bytes-$2 WHERE user_id=$1").bind(user_id).bind(released).execute(&mut *transaction).await.map_err(storage)?;
    }

    ensure_capacity(limit, used, reserved, size_bytes)?;
    insert_blob_object(&mut transaction, hash, size_bytes).await?;
    sqlx::query("INSERT INTO user_blob_charge(user_id, content_hash) VALUES ($1, $2)").bind(user_id).bind(hash.as_str()).execute(&mut *transaction).await.map_err(storage)?;
    sqlx::query("UPDATE user_storage_account SET used_bytes=used_bytes+$2 WHERE user_id=$1").bind(user_id).bind(size_bytes).execute(&mut *transaction).await.map_err(storage)?;
    transaction.commit().await.map_err(storage)
}

pub(super) async fn reserve_book_upload(pool: &PgPool, user_id: &str, library_id: &LibraryId, hash: &ContentHash, size_bytes: u64) -> Result<BookUploadAdmission, AssetError> {
    let size_i64 = size_i64(size_bytes)?;
    let mut transaction = pool.begin().await.map_err(storage)?;
    lock_library_and_require_hash(&mut transaction, user_id, library_id, hash).await?;
    let (limit, used, mut reserved) = lock_account(&mut transaction, user_id).await?;
    reserved -= expire_reservations(&mut transaction, user_id).await?;
    if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM user_blob_charge WHERE user_id=$1 AND content_hash=$2)").bind(user_id).bind(hash.as_str()).fetch_one(&mut *transaction).await.map_err(storage)? {
        transaction.commit().await.map_err(storage)?;
        return Ok(BookUploadAdmission::AlreadyOwned);
    }
    if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM book_upload_reservation WHERE user_id=$1 AND content_hash=$2)").bind(user_id).bind(hash.as_str()).fetch_one(&mut *transaction).await.map_err(storage)? {
        return Err(AssetError::UploadInProgress);
    }
    ensure_capacity(limit, used, reserved, size_i64)?;
    let reservation = BookUploadReservation { id: uuid::Uuid::new_v4().to_string(), user_id: user_id.to_string(), library_id: library_id.clone(), hash: hash.clone(), expected_size_bytes: size_bytes };
    sqlx::query(
        "INSERT INTO book_upload_reservation(id, user_id, library_id, content_hash, size_bytes, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&reservation.id)
    .bind(user_id)
    .bind(library_id.as_str())
    .bind(hash.as_str())
    .bind(size_i64)
    .bind(Utc::now() + UPLOAD_RESERVATION_TTL)
    .execute(&mut *transaction)
    .await
    .map_err(storage)?;
    sqlx::query("UPDATE user_storage_account SET reserved_bytes=reserved_bytes+$2 WHERE user_id=$1").bind(user_id).bind(size_i64).execute(&mut *transaction).await.map_err(storage)?;
    transaction.commit().await.map_err(storage)?;
    Ok(BookUploadAdmission::Reserved(reservation))
}

pub(super) async fn finish_book_upload(pool: &PgPool, reservation: &BookUploadReservation, actual_size_bytes: u64) -> Result<(), AssetError> {
    if actual_size_bytes != reservation.expected_size_bytes {
        return Err(AssetError::LengthMismatch { expected: reservation.expected_size_bytes, actual: actual_size_bytes });
    }
    let actual_size_i64 = size_i64(actual_size_bytes)?;
    let mut transaction = pool.begin().await.map_err(storage)?;
    if let Err(error) = lock_library_and_require_hash(&mut transaction, &reservation.user_id, &reservation.library_id, &reservation.hash).await {
        drop(transaction);
        abort_book_upload(pool, reservation).await?;
        return Err(error);
    }
    let _ = lock_account(&mut transaction, &reservation.user_id).await?;
    let stored_size = sqlx::query_scalar::<_, i64>(
        "SELECT size_bytes FROM book_upload_reservation
         WHERE id=$1 AND user_id=$2 AND library_id=$3 AND content_hash=$4
         FOR UPDATE",
    )
    .bind(&reservation.id)
    .bind(&reservation.user_id)
    .bind(reservation.library_id.as_str())
    .bind(reservation.hash.as_str())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(storage)?;
    let Some(stored_size) = stored_size else {
        let already_charged = sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM user_blob_charge WHERE user_id=$1 AND content_hash=$2)")
            .bind(&reservation.user_id)
            .bind(reservation.hash.as_str())
            .fetch_one(&mut *transaction)
            .await
            .map_err(storage)?;
        if already_charged {
            transaction.commit().await.map_err(storage)?;
            return Ok(());
        }
        return Err(AssetError::Storage("book upload reservation no longer exists".to_string()));
    };
    if stored_size != actual_size_i64 {
        return Err(AssetError::LengthMismatch { expected: stored_size.max(0) as u64, actual: actual_size_bytes });
    }
    insert_blob_object(&mut transaction, &reservation.hash, actual_size_i64).await?;
    let inserted = sqlx::query(
        "INSERT INTO user_blob_charge(user_id, content_hash) VALUES ($1, $2)
         ON CONFLICT (user_id, content_hash) DO NOTHING",
    )
    .bind(&reservation.user_id)
    .bind(reservation.hash.as_str())
    .execute(&mut *transaction)
    .await
    .map_err(storage)?
    .rows_affected();
    sqlx::query(
        "UPDATE user_storage_account
         SET reserved_bytes=reserved_bytes-$2,
             used_bytes=used_bytes+CASE WHEN $3 THEN $2 ELSE 0 END
         WHERE user_id=$1",
    )
    .bind(&reservation.user_id)
    .bind(stored_size)
    .bind(inserted == 1)
    .execute(&mut *transaction)
    .await
    .map_err(storage)?;
    sqlx::query("DELETE FROM book_upload_reservation WHERE id=$1").bind(&reservation.id).execute(&mut *transaction).await.map_err(storage)?;
    transaction.commit().await.map_err(storage)
}

pub(super) async fn abort_book_upload(pool: &PgPool, reservation: &BookUploadReservation) -> Result<(), AssetError> {
    let mut transaction = pool.begin().await.map_err(storage)?;
    let _ = lock_account(&mut transaction, &reservation.user_id).await?;
    let released =
        sqlx::query_scalar::<_, i64>("DELETE FROM book_upload_reservation WHERE id=$1 AND user_id=$2 RETURNING size_bytes").bind(&reservation.id).bind(&reservation.user_id).fetch_optional(&mut *transaction).await.map_err(storage)?;
    if let Some(released) = released {
        sqlx::query("UPDATE user_storage_account SET reserved_bytes=reserved_bytes-$2 WHERE user_id=$1").bind(&reservation.user_id).bind(released).execute(&mut *transaction).await.map_err(storage)?;
    }
    transaction.commit().await.map_err(storage)
}

pub(super) async fn reconcile_user_book_charges(transaction: &mut Transaction<'_, Postgres>, user_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO user_storage_account(user_id) VALUES ($1) ON CONFLICT (user_id) DO NOTHING").bind(user_id).execute(&mut **transaction).await?;
    sqlx::query("SELECT user_id FROM user_storage_account WHERE user_id=$1 FOR UPDATE").bind(user_id).fetch_one(&mut **transaction).await?;
    let released = sqlx::query_scalar::<_, i64>(
        "WITH latest_per_occupant AS (
             SELECT DISTINCT ON (library_id, entity_id, data->>'dir_id', data->>'file_name')
                 library_id,
                 data->>'dir_id' AS dir_id,
                 data->>'file_name' AS file_name,
                 event_type,
                 data->>'content_hash' AS content_hash,
                 hlc_pt, hlc_l, version_rank, client_id, client_seq, event_id
             FROM library_events
             WHERE user_id=$1 AND event_type IN ('book_dir_added', 'book_dir_removed')
             ORDER BY library_id, entity_id, data->>'dir_id', data->>'file_name',
                      hlc_pt DESC, hlc_l DESC, version_rank DESC,
                      client_id COLLATE \"C\" DESC, client_seq DESC, event_id COLLATE \"C\" DESC
         ), latest_per_path AS (
             SELECT DISTINCT ON (library_id, dir_id, file_name) content_hash
             FROM latest_per_occupant
             WHERE event_type='book_dir_added'
             ORDER BY library_id, dir_id, file_name,
                      hlc_pt DESC, hlc_l DESC, version_rank DESC,
                      client_id COLLATE \"C\" DESC, client_seq DESC, event_id COLLATE \"C\" DESC
         ), removed AS (
             DELETE FROM user_blob_charge AS charge
             USING blob_object AS object
             WHERE charge.user_id=$1
               AND object.content_hash=charge.content_hash
               AND NOT EXISTS (SELECT 1 FROM latest_per_path WHERE content_hash=charge.content_hash)
             RETURNING object.size_bytes
         )
         SELECT COALESCE(SUM(size_bytes), 0)::BIGINT FROM removed",
    )
    .bind(user_id)
    .fetch_one(&mut **transaction)
    .await?;
    if released > 0 {
        sqlx::query("UPDATE user_storage_account SET used_bytes=GREATEST(used_bytes-$2, 0) WHERE user_id=$1").bind(user_id).bind(released).execute(&mut **transaction).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PostgresDatabase;
    use sqlx::postgres::PgPoolOptions;

    fn hash() -> ContentHash {
        let raw = uuid::Uuid::new_v4().simple().to_string().repeat(2);
        ContentHash::parse(&raw).unwrap()
    }

    async fn seed_user(pool: &PgPool, quota: i64) -> (String, LibraryId) {
        let user_id = uuid::Uuid::new_v4().to_string();
        let username = format!("quota-{}", uuid::Uuid::new_v4().simple());
        let library_id = LibraryId::parse(&format!("library-{}", uuid::Uuid::new_v4())).unwrap();
        sqlx::query("INSERT INTO users(id, username) VALUES ($1, $2)").bind(&user_id).bind(username).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id, user_id) VALUES ($1, $2)").bind(library_id.as_str()).bind(&user_id).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO user_storage_account(user_id, quota_bytes) VALUES ($1, $2)").bind(&user_id).bind(quota).execute(pool).await.unwrap();
        (user_id, library_id)
    }

    async fn reference(pool: &PgPool, user_id: &str, library_id: &LibraryId, hash: &ContentHash, client_seq: i64) {
        sqlx::query(
            "INSERT INTO library_events(
                 library_id, user_id, event_type, entity_id, data, client_id,
                 timestamp, client_seq, hlc_pt, hlc_l, version_rank, event_id
             ) VALUES ($1, $2, 'book_dir_added', $3, $4, $5, 1, $6, $6, 0, 0, $7)",
        )
        .bind(library_id.as_str())
        .bind(user_id)
        .bind(format!("book-{client_seq}"))
        .bind(serde_json::json!({ "dir_id": "root", "file_name": format!("{client_seq}.epub"), "content_hash": hash.as_str() }))
        .bind(format!("client-{client_seq}"))
        .bind(client_seq)
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(pool)
        .await
        .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn quota_is_transactional_deduplicated_per_user_and_reservations_release() {
        let database_url = std::env::var("SYNC_E2E_DATABASE_URL").expect("SYNC_E2E_DATABASE_URL must be set");
        let pool = PgPoolOptions::new().max_connections(8).connect(&database_url).await.unwrap();
        crate::migrate(&PostgresDatabase::from_pool(pool.clone())).await.unwrap();

        let shared = hash();
        let (first_user, first_library) = seed_user(&pool, 100).await;
        let (second_user, second_library) = seed_user(&pool, 100).await;
        reference(&pool, &first_user, &first_library, &shared, 1).await;
        reference(&pool, &second_user, &second_library, &shared, 1).await;

        claim_existing_book(&pool, &first_user, &first_library, &shared, 60).await.unwrap();
        claim_existing_book(&pool, &first_user, &first_library, &shared, 60).await.unwrap();
        claim_existing_book(&pool, &second_user, &second_library, &shared, 60).await.unwrap();
        let first_used: i64 = sqlx::query_scalar("SELECT used_bytes FROM user_storage_account WHERE user_id=$1").bind(&first_user).fetch_one(&pool).await.unwrap();
        let second_used: i64 = sqlx::query_scalar("SELECT used_bytes FROM user_storage_account WHERE user_id=$1").bind(&second_user).fetch_one(&pool).await.unwrap();
        let physical_objects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM blob_object WHERE content_hash=$1").bind(shared.as_str()).fetch_one(&pool).await.unwrap();
        assert_eq!((first_used, second_used, physical_objects), (60, 60, 1));

        let too_large = hash();
        reference(&pool, &first_user, &first_library, &too_large, 2).await;
        assert!(matches!(claim_existing_book(&pool, &first_user, &first_library, &too_large, 41).await, Err(AssetError::QuotaExceeded { .. })));

        let (concurrent_user, concurrent_library) = seed_user(&pool, 100).await;
        let left = hash();
        let right = hash();
        reference(&pool, &concurrent_user, &concurrent_library, &left, 1).await;
        reference(&pool, &concurrent_user, &concurrent_library, &right, 2).await;
        let left_attempt = reserve_book_upload(&pool, &concurrent_user, &concurrent_library, &left, 60);
        let right_attempt = reserve_book_upload(&pool, &concurrent_user, &concurrent_library, &right, 60);
        let (left_result, right_result) = tokio::join!(left_attempt, right_attempt);
        let reservation = match (left_result, right_result) {
            (Ok(BookUploadAdmission::Reserved(reservation)), Err(AssetError::QuotaExceeded { .. })) | (Err(AssetError::QuotaExceeded { .. }), Ok(BookUploadAdmission::Reserved(reservation))) => reservation,
            results => panic!("exactly one concurrent reservation must fit: {results:?}"),
        };
        finish_book_upload(&pool, &reservation, 60).await.unwrap();
        let account: (i64, i64) = sqlx::query_as("SELECT used_bytes, reserved_bytes FROM user_storage_account WHERE user_id=$1").bind(&concurrent_user).fetch_one(&pool).await.unwrap();
        assert_eq!(account, (60, 0));

        let placement_seq = if reservation.hash == left { 1_i64 } else { 2_i64 };
        sqlx::query(
            "INSERT INTO library_events(
                 library_id, user_id, event_type, entity_id, data, client_id,
                 timestamp, client_seq, hlc_pt, hlc_l, version_rank, event_id
             ) VALUES ($1, $2, 'book_dir_removed', $3, $4, 'remover', 2, 99, 99, 0, 1, $5)",
        )
        .bind(concurrent_library.as_str())
        .bind(&concurrent_user)
        .bind(format!("book-{placement_seq}"))
        .bind(serde_json::json!({ "dir_id": "root", "file_name": format!("{placement_seq}.epub") }))
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(&pool)
        .await
        .unwrap();
        let mut reconciliation = pool.begin().await.unwrap();
        reconcile_user_book_charges(&mut reconciliation, &concurrent_user).await.unwrap();
        reconciliation.commit().await.unwrap();
        let used_after_removal: i64 = sqlx::query_scalar("SELECT used_bytes FROM user_storage_account WHERE user_id=$1").bind(&concurrent_user).fetch_one(&pool).await.unwrap();
        assert_eq!(used_after_removal, 0);

        let abort_hash = if reservation.hash == left { right } else { left };
        let abort_reservation = match reserve_book_upload(&pool, &concurrent_user, &concurrent_library, &abort_hash, 60).await.unwrap() {
            BookUploadAdmission::Reserved(reservation) => reservation,
            BookUploadAdmission::AlreadyOwned => panic!("the unuploaded hash must not already be charged"),
        };
        abort_book_upload(&pool, &abort_reservation).await.unwrap();
        let reserved_after_abort: i64 = sqlx::query_scalar("SELECT reserved_bytes FROM user_storage_account WHERE user_id=$1").bind(&concurrent_user).fetch_one(&pool).await.unwrap();
        assert_eq!(reserved_after_abort, 0);

        sqlx::query("DELETE FROM users WHERE id=ANY($1)").bind(vec![first_user, second_user, concurrent_user]).execute(&pool).await.unwrap();
    }
}
