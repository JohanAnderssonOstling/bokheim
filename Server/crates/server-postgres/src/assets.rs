use chrono::{Duration, Utc};
use server_core::{AssetError, BookUploadAdmission, BookUploadReservation};
use sqlx::{PgPool, Postgres, Row, Transaction};
use sync_common::api::assets::{BlobManifestEntry, BlobManifestRejectionReason, BlobManifestResponse, RejectedBlobManifestEntry};
#[cfg(test)]
use sync_common::DirId;
use sync_common::{ContentHash, LibraryId};

const UPLOAD_RESERVATION_TTL: Duration = Duration::minutes(30);

fn storage(error: impl std::fmt::Display) -> AssetError {
    AssetError::Storage(error.to_string())
}

fn size_i64(size: u64) -> Result<i64, AssetError> {
    i64::try_from(size).map_err(|_| AssetError::Storage("book size does not fit the quota ledger".to_string()))
}

async fn require_library_owner(transaction: &mut Transaction<'_, Postgres>, user_id: &str, library_id: &LibraryId) -> Result<(), AssetError> {
    let library_id = library_id.as_str();
    let authorized = sqlx::query_file_scalar!("src/sql/assets/library_owned_by_user.sql", library_id, user_id).fetch_one(&mut **transaction).await.map_err(storage)?.unwrap_or(false);
    if authorized {
        Ok(())
    } else {
        Err(AssetError::Forbidden)
    }
}

async fn account_snapshot(transaction: &mut Transaction<'_, Postgres>, user_id: &str) -> Result<(i64, i64, i64), AssetError> {
    sqlx::query_file!("src/sql/account/ensure_exists.sql", user_id).execute(&mut **transaction).await.map_err(storage)?;
    let row = sqlx::query(include_str!("sql/account/select.sql")).bind(user_id).fetch_one(&mut **transaction).await.map_err(storage)?;
    Ok((row.try_get("quota_bytes").map_err(storage)?, row.try_get("used_bytes").map_err(storage)?, row.try_get("reserved_bytes").map_err(storage)?))
}

async fn expire_reservations(transaction: &mut Transaction<'_, Postgres>, user_id: &str) -> Result<i64, AssetError> {
    let released = sqlx::query_file_scalar!("src/sql/assets/expire_reservations.sql", user_id).fetch_one(&mut **transaction).await.map_err(storage)?.unwrap_or(0);
    if released > 0 {
        sqlx::query_file!("src/sql/assets/adjust_reserved.sql", user_id, -released).execute(&mut **transaction).await.map_err(storage)?;
    }
    Ok(released)
}

async fn insert_blob_object(transaction: &mut Transaction<'_, Postgres>, hash: &ContentHash, size_bytes: i64) -> Result<(), AssetError> {
    let hash_str = hash.as_str();
    let inserted = sqlx::query_file_scalar!("src/sql/assets/insert_blob_object.sql", hash_str, size_bytes).fetch_optional(&mut **transaction).await.map_err(storage)?;
    if inserted.is_some() {
        Ok(())
    } else {
        Err(AssetError::Storage(format!("stored size for content hash {hash} is inconsistent")))
    }
}

pub(super) async fn negotiate_books(pool: &PgPool, user_id: &str, library_id: &LibraryId, blobs: &[BlobManifestEntry]) -> Result<BlobManifestResponse, AssetError> {
    let mut seen = std::collections::HashSet::with_capacity(blobs.len());
    if blobs.iter().any(|blob| !seen.insert(blob.content_hash.clone())) {
        return Err(AssetError::InvalidInput("book manifest contains a duplicate content hash".to_string()));
    }

    let mut response = BlobManifestResponse::default();
    let mut hashes = Vec::with_capacity(blobs.len());
    let mut sizes = Vec::with_capacity(blobs.len());
    for blob in blobs {
        if blob.size_bytes == 0 || blob.size_bytes > server_core::AssetKind::Book.max_bytes() {
            response.rejected.push(RejectedBlobManifestEntry { content_hash: blob.content_hash.clone(), reason: BlobManifestRejectionReason::InvalidSize });
            continue;
        }
        hashes.push(blob.content_hash.as_str().to_string());
        sizes.push(size_i64(blob.size_bytes)?);
    }
    if hashes.is_empty() {
        return Ok(response);
    }

    let mut transaction = pool.begin().await.map_err(storage)?;
    require_library_owner(&mut transaction, user_id, library_id).await?;
    let (limit, used, mut reserved) = account_snapshot(&mut transaction, user_id).await?;
    let released = expire_reservations(&mut transaction, user_id).await?;
    reserved -= released;
    if released > 0 {
        sqlx::query_file!("src/sql/assets/adjust_reserved.sql", user_id, -released).execute(&mut *transaction).await.map_err(storage)?;
    }
    let rows = sqlx::query_file!("src/sql/assets/inspect_book_manifest.sql", &hashes, &sizes, user_id).fetch_all(&mut *transaction).await.map_err(storage)?;

    let mut available = limit.saturating_sub(used).saturating_sub(reserved).max(0);
    let mut claimed_hashes = Vec::new();
    let mut claimed_sizes = Vec::new();
    for row in rows {
        let (raw_hash, requested_size, stored_size, owned, reservation_id, reservation_library_id, reservation_size) =
            (row.content_hash, row.size_bytes, row.stored_size_bytes, row.owned, row.reservation_id, row.reservation_library_id, row.reservation_size_bytes);
        let hash = ContentHash::parse(&raw_hash).map_err(|error| AssetError::Storage(format!("stored manifest hash is invalid: {error}")))?;
        if owned {
            response.owned.push(hash);
            continue;
        }
        if reservation_id.is_some() {
            if reservation_size != Some(requested_size) {
                response.rejected.push(RejectedBlobManifestEntry { content_hash: hash, reason: BlobManifestRejectionReason::SizeMismatch });
            } else if reservation_library_id.as_deref() == Some(library_id.as_str()) {
                response.upload.push(hash);
            } else {
                response.rejected.push(RejectedBlobManifestEntry { content_hash: hash, reason: BlobManifestRejectionReason::UploadInProgress });
            }
            continue;
        }
        if let Some(stored_size) = stored_size {
            if stored_size != requested_size {
                response.rejected.push(RejectedBlobManifestEntry { content_hash: hash, reason: BlobManifestRejectionReason::SizeMismatch });
            } else if requested_size > available {
                response.rejected.push(RejectedBlobManifestEntry { content_hash: hash, reason: BlobManifestRejectionReason::QuotaExceeded });
            } else {
                available -= requested_size;
                claimed_hashes.push(raw_hash);
                claimed_sizes.push(requested_size);
                response.claimed.push(hash);
            }
            continue;
        }
        if requested_size > available {
            response.rejected.push(RejectedBlobManifestEntry { content_hash: hash, reason: BlobManifestRejectionReason::QuotaExceeded });
        } else {
            // This is an admission preview. The ordinary PUT reservation makes
            // the quota decision durable immediately before bytes are accepted.
            available -= requested_size;
            response.upload.push(hash);
        }
    }

    if !claimed_hashes.is_empty() {
        let row = sqlx::query(include_str!("sql/assets/claim_book_manifest.sql"))
            .bind(user_id)
            .bind(&claimed_hashes)
            .bind(&claimed_sizes)
            .fetch_one(&mut *transaction)
            .await
            .map_err(storage)?;
        let accepted: bool = row.try_get("accepted").map_err(storage)?;
        if !accepted {
            return Err(AssetError::QuotaExceeded { limit: limit.max(0) as u64, used: used.max(0) as u64, reserved: reserved.max(0) as u64, requested: claimed_sizes.iter().sum::<i64>().max(0) as u64 });
        }
    }
    transaction.commit().await.map_err(storage)?;
    Ok(response)
}

pub(super) async fn claim_existing_book(pool: &PgPool, user_id: &str, library_id: &LibraryId, hash: &ContentHash, size_bytes: u64) -> Result<(), AssetError> {
    let size_bytes = size_i64(size_bytes)?;
    let mut transaction = pool.begin().await.map_err(storage)?;
    require_library_owner(&mut transaction, user_id, library_id).await?;
    let _ = account_snapshot(&mut transaction, user_id).await?;
    let released = expire_reservations(&mut transaction, user_id).await?;
    if released > 0 {
        sqlx::query_file!("src/sql/assets/adjust_reserved.sql", user_id, -released).execute(&mut *transaction).await.map_err(storage)?;
    }

    let hash_str = hash.as_str();
    if sqlx::query_file_scalar!("src/sql/assets/charge_exists.sql", user_id, hash_str).fetch_one(&mut *transaction).await.map_err(storage)?.unwrap_or(false) {
        transaction.commit().await.map_err(storage)?;
        return Ok(());
    }

    let active_reservation = sqlx::query_file_scalar!("src/sql/assets/delete_reservation_by_content_returning_size.sql", user_id, hash_str).fetch_optional(&mut *transaction).await.map_err(storage)?;
    if let Some(released) = active_reservation {
        sqlx::query_file!("src/sql/assets/adjust_reserved.sql", user_id, -released).execute(&mut *transaction).await.map_err(storage)?;
    }

    insert_blob_object(&mut transaction, hash, size_bytes).await?;
    let charged = sqlx::query_file!("src/sql/assets/insert_charge_idempotent.sql", user_id, hash_str).execute(&mut *transaction).await.map_err(storage)?.rows_affected() == 1;
    if charged {
        let account = sqlx::query(include_str!("sql/assets/claim_used_capacity.sql")).bind(user_id).bind(size_bytes).fetch_optional(&mut *transaction).await.map_err(storage)?;
        if account.is_none() {
            sqlx::query(include_str!("sql/assets/delete_charge.sql")).bind(user_id).bind(hash_str).execute(&mut *transaction).await.map_err(storage)?;
            let (limit, used, reserved) = account_snapshot(&mut transaction, user_id).await?;
            return Err(AssetError::QuotaExceeded { limit: limit.max(0) as u64, used: used.max(0) as u64, reserved: reserved.max(0) as u64, requested: size_bytes as u64 });
        }
    }
    transaction.commit().await.map_err(storage)
}

pub(super) async fn reserve_book_upload(pool: &PgPool, user_id: &str, library_id: &LibraryId, hash: &ContentHash, size_bytes: u64) -> Result<BookUploadAdmission, AssetError> {
    let size_i64 = size_i64(size_bytes)?;
    let mut transaction = pool.begin().await.map_err(storage)?;
    require_library_owner(&mut transaction, user_id, library_id).await?;
    let _ = account_snapshot(&mut transaction, user_id).await?;
    let released = expire_reservations(&mut transaction, user_id).await?;
    if released > 0 {
        sqlx::query_file!("src/sql/assets/adjust_reserved.sql", user_id, -released).execute(&mut *transaction).await.map_err(storage)?;
    }
    let hash_str = hash.as_str();
    if sqlx::query_file_scalar!("src/sql/assets/charge_exists.sql", user_id, hash_str).fetch_one(&mut *transaction).await.map_err(storage)?.unwrap_or(false) {
        transaction.commit().await.map_err(storage)?;
        return Ok(BookUploadAdmission::AlreadyOwned);
    }
    if sqlx::query_file_scalar!("src/sql/assets/reservation_exists.sql", user_id, hash_str).fetch_one(&mut *transaction).await.map_err(storage)?.unwrap_or(false) {
        return Err(AssetError::UploadInProgress);
    }
    let reserved = sqlx::query(include_str!("sql/assets/reserve_capacity.sql")).bind(user_id).bind(size_i64).fetch_optional(&mut *transaction).await.map_err(storage)?;
    if reserved.is_none() {
        let (limit, used, reserved) = account_snapshot(&mut transaction, user_id).await?;
        return Err(AssetError::QuotaExceeded { limit: limit.max(0) as u64, used: used.max(0) as u64, reserved: reserved.max(0) as u64, requested: size_bytes });
    }
    let reservation = BookUploadReservation { id: uuid::Uuid::new_v4().to_string(), user_id: user_id.to_string(), library_id: library_id.clone(), hash: hash.clone(), expected_size_bytes: size_bytes };
    let library_id_str = library_id.as_str();
    let expires_at = Utc::now() + UPLOAD_RESERVATION_TTL;
    let inserted = sqlx::query_file!("src/sql/assets/insert_reservation.sql", reservation.id, user_id, library_id_str, hash_str, size_i64, expires_at).execute(&mut *transaction).await;
    if let Err(error) = inserted {
        sqlx::query(include_str!("sql/assets/release_reserved_capacity.sql")).bind(user_id).bind(size_i64).execute(&mut *transaction).await.map_err(storage)?;
        if error.as_database_error().and_then(|database| database.code()).as_deref() == Some("23505") {
            return Err(AssetError::UploadInProgress);
        }
        return Err(storage(error));
    }
    transaction.commit().await.map_err(storage)?;
    Ok(BookUploadAdmission::Reserved(reservation))
}

pub(super) async fn finish_book_upload(pool: &PgPool, reservation: &BookUploadReservation, actual_size_bytes: u64) -> Result<(), AssetError> {
    if actual_size_bytes != reservation.expected_size_bytes {
        return Err(AssetError::LengthMismatch { expected: reservation.expected_size_bytes, actual: actual_size_bytes });
    }
    let actual_size_i64 = size_i64(actual_size_bytes)?;
    let mut transaction = pool.begin().await.map_err(storage)?;
    if let Err(error) = require_library_owner(&mut transaction, &reservation.user_id, &reservation.library_id).await {
        drop(transaction);
        abort_book_upload(pool, reservation).await?;
        return Err(error);
    }
    let _ = account_snapshot(&mut transaction, &reservation.user_id).await?;
    let reservation_hash = reservation.hash.as_str();
    let stored_size: Option<i64> =
        sqlx::query_scalar(include_str!("sql/assets/delete_reservation_by_id_and_user_returning_size.sql")).bind(&reservation.id).bind(&reservation.user_id).fetch_optional(&mut *transaction).await.map_err(storage)?;
    let Some(stored_size) = stored_size else {
        let already_charged = sqlx::query_file_scalar!("src/sql/assets/charge_exists.sql", reservation.user_id, reservation_hash).fetch_one(&mut *transaction).await.map_err(storage)?.unwrap_or(false);
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
    let inserted = sqlx::query_file!("src/sql/assets/insert_charge_idempotent.sql", reservation.user_id, reservation_hash).execute(&mut *transaction).await.map_err(storage)?.rows_affected();
    let charged = inserted == 1;
    sqlx::query_file!("src/sql/assets/finish_upload_update_account.sql", reservation.user_id, stored_size, charged).execute(&mut *transaction).await.map_err(storage)?;
    transaction.commit().await.map_err(storage)
}

pub(super) async fn abort_book_upload(pool: &PgPool, reservation: &BookUploadReservation) -> Result<(), AssetError> {
    let mut transaction = pool.begin().await.map_err(storage)?;
    let _ = account_snapshot(&mut transaction, &reservation.user_id).await?;
    let released = sqlx::query_file_scalar!("src/sql/assets/delete_reservation_by_id_and_user_returning_size.sql", reservation.id, reservation.user_id).fetch_optional(&mut *transaction).await.map_err(storage)?;
    if let Some(released) = released {
        sqlx::query_file!("src/sql/assets/adjust_reserved.sql", reservation.user_id, -released).execute(&mut *transaction).await.map_err(storage)?;
    }
    transaction.commit().await.map_err(storage)
}

/// Applies reference-count changes atomically with a winning lifecycle write.
/// Placement mutations are deliberately absent: a book may be present in any
/// number of directories but owns exactly one content blob reference.
pub(super) async fn apply_lifecycle_reference_deltas(transaction: &mut Transaction<'_, Postgres>, user_id: &str, deltas: impl IntoIterator<Item = (ContentHash, i64)>) -> Result<(), sqlx::Error> {
    let mut merged = std::collections::BTreeMap::<String, i64>::new();
    for (hash, delta) in deltas {
        if delta != 0 {
            *merged.entry(hash.into_string()).or_default() += delta;
        }
    }
    let (hashes, values): (Vec<_>, Vec<_>) = merged.into_iter().filter(|(_, delta)| *delta != 0).unzip();
    if hashes.is_empty() {
        return Ok(());
    }
    let row = sqlx::query(include_str!("sql/assets/apply_blob_reference_deltas.sql")).bind(user_id).bind(&hashes).bind(&values).fetch_one(&mut **transaction).await?;
    let valid: bool = row.try_get("valid")?;
    if !valid {
        return Err(sqlx::Error::Protocol("lifecycle blob reference underflow".to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn apply_reference_deltas(projection: &mut Transaction<'_, Postgres>, user_id: &str, deltas: impl IntoIterator<Item = (ContentHash, i64)>) -> Result<(), AssetError> {
        let mut hashes = Vec::new();
        let mut values = Vec::new();
        for (hash, delta) in deltas {
            if delta == 0 {
                continue;
            }
            hashes.push(hash.as_str().to_string());
            values.push(delta);
        }
        if hashes.is_empty() {
            return Ok(());
        }

        let row = sqlx::query(include_str!("sql/assets/apply_blob_reference_deltas.sql")).bind(user_id).bind(&hashes).bind(&values).fetch_one(&mut **projection).await.map_err(storage)?;
        let valid: bool = row.try_get("valid").map_err(storage)?;
        let applied: i64 = row.try_get("applied").map_err(storage)?;
        if !valid || applied < 0 {
            return Err(AssetError::Storage("blob reference delta update was rejected".to_string()));
        }

        Ok(())
    }

    fn hash() -> ContentHash {
        let raw = uuid::Uuid::new_v4().simple().to_string().repeat(2);
        ContentHash::parse(&raw).unwrap()
    }

    async fn seed_user(pool: &PgPool, quota: i64) -> (String, LibraryId) {
        let user_id = uuid::Uuid::new_v4().to_string();
        let email = format!("quota-{}@example.com", uuid::Uuid::new_v4().simple());
        let library_id = LibraryId::parse(&format!("library-{}", uuid::Uuid::new_v4())).unwrap();
        sqlx::query("INSERT INTO users(id, email) VALUES ($1, $2)").bind(&user_id).bind(email).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO libraries(id, user_id) VALUES ($1, $2)").bind(library_id.as_str()).bind(&user_id).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO user_storage_account(user_id, quota_bytes) VALUES ($1, $2)").bind(&user_id).bind(quota).execute(pool).await.unwrap();
        (user_id, library_id)
    }

    async fn reference(pool: &PgPool, _user_id: &str, library_id: &LibraryId, hash: &ContentHash, client_seq: i64) {
        sqlx::query(
            "INSERT INTO sync_state(
                 library_id,kind,entity_key,entity_subkey,value,present,content_hash,
                 changed_at,client_id,client_seq,version_rank,event_id
             ) VALUES($1,'book_lifecycle',$2,'',$3,TRUE,$4,1,$5,$6,0,$7)",
        )
        .bind(library_id.as_str())
        .bind(format!("book-{client_seq}"))
        .bind(Vec::<u8>::new())
        .bind(hash.as_str())
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
        let Some(pool) = crate::test_support::isolated_pool("assets", 8).await else { return };

        let shared = hash();
        let (first_user, first_library) = seed_user(&pool, 100).await;
        let (second_user, second_library) = seed_user(&pool, 100).await;

        claim_existing_book(&pool, &first_user, &first_library, &shared, 60).await.unwrap();
        claim_existing_book(&pool, &first_user, &first_library, &shared, 60).await.unwrap();
        assert!(matches!(claim_existing_book(&pool, &second_user, &first_library, &shared, 60).await, Err(AssetError::Forbidden)));
        claim_existing_book(&pool, &second_user, &second_library, &shared, 60).await.unwrap();
        let first_used: i64 = sqlx::query_scalar("SELECT used_bytes FROM user_storage_account WHERE user_id=$1").bind(&first_user).fetch_one(&pool).await.unwrap();
        let second_used: i64 = sqlx::query_scalar("SELECT used_bytes FROM user_storage_account WHERE user_id=$1").bind(&second_user).fetch_one(&pool).await.unwrap();
        let physical_objects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM blob_object WHERE content_hash=$1").bind(shared.as_str()).fetch_one(&pool).await.unwrap();
        assert_eq!((first_used, second_used, physical_objects), (60, 60, 1));

        let too_large = hash();
        assert!(matches!(claim_existing_book(&pool, &first_user, &first_library, &too_large, 41).await, Err(AssetError::QuotaExceeded { .. })));

        let (concurrent_user, concurrent_library) = seed_user(&pool, 100).await;
        let left = hash();
        let right = hash();
        reference(&pool, &concurrent_user, &concurrent_library, &left, 1).await;
        reference(&pool, &concurrent_user, &concurrent_library, &right, 2).await;
        let mut initial_projection = pool.begin().await.unwrap();
        apply_reference_deltas(&mut initial_projection, &concurrent_user, [(left.clone(), 1), (right.clone(), 1)]).await.unwrap();
        initial_projection.commit().await.unwrap();
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
            "UPDATE sync_state
             SET present=FALSE, content_hash=NULL, changed_at=2, client_id='remover',client_seq=99,
                 version_rank=1, event_id=$3, server_seq=DEFAULT
             WHERE library_id=$1 AND kind='book_lifecycle' AND entity_key=$2 AND entity_subkey=''",
        )
        .bind(concurrent_library.as_str())
        .bind(format!("book-{placement_seq}"))
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(&pool)
        .await
        .unwrap();
        let mut reconciliation = pool.begin().await.unwrap();
        apply_reference_deltas(&mut reconciliation, &concurrent_user, [(reservation.hash.clone(), -1)]).await.unwrap();
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

    #[tokio::test]
    #[ignore = "requires SYNC_E2E_DATABASE_URL"]
    async fn book_manifest_claims_stored_blobs_in_one_account_transaction_and_classifies_every_hash() {
        let Some(pool) = crate::test_support::isolated_pool("assets", 4).await else { return };
        let (user_id, library_id) = seed_user(&pool, 100).await;
        let owned = hash();
        let claimable = hash();
        let missing = hash();
        let mismatch = hash();
        let over_quota = hash();

        claim_existing_book(&pool, &user_id, &library_id, &owned, 10).await.unwrap();
        for (content_hash, size) in [(&claimable, 20_i64), (&mismatch, 5_i64), (&over_quota, 80_i64)] {
            sqlx::query("INSERT INTO blob_object(content_hash, size_bytes) VALUES ($1, $2)").bind(content_hash.as_str()).bind(size).execute(&pool).await.unwrap();
        }
        let response = negotiate_books(
            &pool,
            &user_id,
            &library_id,
            &[
                BlobManifestEntry { content_hash: owned.clone(), size_bytes: 10 },
                BlobManifestEntry { content_hash: claimable.clone(), size_bytes: 20 },
                BlobManifestEntry { content_hash: missing.clone(), size_bytes: 30 },
                BlobManifestEntry { content_hash: mismatch.clone(), size_bytes: 6 },
                BlobManifestEntry { content_hash: over_quota.clone(), size_bytes: 80 },
            ],
        )
        .await
        .unwrap();

        assert_eq!(response.owned, vec![owned]);
        assert_eq!(response.claimed, vec![claimable]);
        assert_eq!(response.upload, vec![missing]);
        assert_eq!(response.rejected.len(), 2);
        assert_eq!(response.rejected[0].content_hash, mismatch);
        assert_eq!(response.rejected[0].reason, BlobManifestRejectionReason::SizeMismatch);
        assert_eq!(response.rejected[1].content_hash, over_quota);
        assert_eq!(response.rejected[1].reason, BlobManifestRejectionReason::QuotaExceeded);

        let account: (i64, i64) = sqlx::query_as("SELECT used_bytes, reserved_bytes FROM user_storage_account WHERE user_id=$1").bind(&user_id).fetch_one(&pool).await.unwrap();
        let charges: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_blob_charge WHERE user_id=$1").bind(&user_id).fetch_one(&pool).await.unwrap();
        assert_eq!(account, (30, 0));
        assert_eq!(charges, 2);

        let duplicate = BlobManifestEntry { content_hash: hash(), size_bytes: 1 };
        assert!(matches!(negotiate_books(&pool, &user_id, &library_id, &[duplicate.clone(), duplicate]).await, Err(AssetError::InvalidInput(_))));
        sqlx::query("DELETE FROM users WHERE id=$1").bind(user_id).execute(&pool).await.unwrap();
    }
}
