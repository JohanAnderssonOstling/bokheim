use crate::cleanup::{lock_publication, repair_unbound_aliases};
use crate::{BookStorageUsage, LibraryStorageUsage, StorageUsage};
use server_asset_store::{AssetError, AssetKind, FsAssetStore, PreparedCatalogBook};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use sync_common::api::assets::{BlobManifestEntry, BlobManifestRejectionReason, BlobManifestResponse, RejectedBlobManifestEntry};
use sync_common::{BookId, LibraryId};
use content_address::ContentHash;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BookUploadReservation {
    pub(crate) id: String,
    pub(crate) user_id: String,
    pub(crate) library_id: LibraryId,
    pub(crate) hash: BookId,
    pub(crate) expected_size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BookUploadAdmission {
    AlreadyOwned,
    Reserved(BookUploadReservation),
}

// Five minutes of grace keeps an active upload from racing reservation expiry
// at its HTTP deadline.
const UPLOAD_RESERVATION_TTL_SECONDS: f64 = (server_asset_store::MAX_BOOK_UPLOAD_DURATION.as_secs() + 5 * 60) as f64;

#[derive(Clone, Copy)]
struct AccountSnapshot {
    limit: i64,
    used: i64,
    reserved: i64,
}

impl AccountSnapshot {
    fn quota_exceeded(self, requested: u64) -> AssetError {
        AssetError::QuotaExceeded { limit: self.limit.max(0) as u64, used: self.used.max(0) as u64, reserved: self.reserved.max(0) as u64, requested }
    }
}

fn storage(error: impl std::fmt::Display) -> AssetError {
    AssetError::Storage(error.to_string())
}

pub(crate) async fn configure_user_storage_quota(database: &crate::PostgresDatabase, user_id: &str, quota_bytes: i64) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO user_storage_account(user_id, quota_bytes) VALUES ($1, $2)
         ON CONFLICT (user_id) DO UPDATE SET quota_bytes=EXCLUDED.quota_bytes
         RETURNING quota_bytes",
    )
    .bind(user_id)
    .bind(quota_bytes)
    .fetch_one(database.pool())
    .await
}

fn size_i64(size: u64) -> Result<i64, AssetError> {
    i64::try_from(size).map_err(|_| AssetError::Storage("asset size does not fit the quota ledger".to_string()))
}

// This query checks ownership, deletion, and cloud access together, retaining
// the library share lock until commit so deletion or disablement cannot pass.
async fn require_cloud_storage(transaction: &mut Transaction<'_, Postgres>, user: &str, library: &LibraryId) -> Result<(), AssetError> {
    let phase_started = Instant::now();
    let enabled: Option<bool> = sqlx::query_scalar(include_str!("sql/cloud_storage/require_enabled.sql")).bind(library.to_string()).bind(user).fetch_optional(&mut **transaction).await.map_err(storage)?;
    tracing::debug!(target: "sync_performance", phase = "asset_library_authorize", elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
    if enabled == Some(true) {
        Ok(())
    } else {
        Err(AssetError::Forbidden)
    }
}

async fn ensure_account(transaction: &mut Transaction<'_, Postgres>, user_id: &str) -> Result<(), AssetError> {
    sqlx::query_file!("src/sql/account/ensure_exists.sql", user_id).execute(&mut **transaction).await.map_err(storage)?;
    Ok(())
}

async fn account_snapshot(transaction: &mut Transaction<'_, Postgres>, user_id: &str) -> Result<AccountSnapshot, AssetError> {
    let phase_started = Instant::now();
    let row = sqlx::query(include_str!("sql/account/select.sql")).bind(user_id).fetch_one(&mut **transaction).await.map_err(storage)?;
    tracing::debug!(target: "sync_performance", phase = "account_snapshot", elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
    Ok(AccountSnapshot { limit: row.try_get("quota_bytes").map_err(storage)?, used: row.try_get("used_bytes").map_err(storage)?, reserved: row.try_get("reserved_bytes").map_err(storage)? })
}

async fn expire_reservations(transaction: &mut Transaction<'_, Postgres>, user_id: &str) -> Result<(), AssetError> {
    sqlx::query(include_str!("sql/assets/expire_reservations.sql")).bind(user_id).execute(&mut **transaction).await.map_err(storage)?;
    Ok(())
}

async fn begin_asset_transaction(pool: &PgPool) -> Result<Transaction<'_, Postgres>, AssetError> {
    let phase_started = Instant::now();
    let transaction = pool.begin().await.map_err(storage)?;
    tracing::debug!(target: "sync_performance", phase = "asset_pool_acquire_and_begin", elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
    Ok(transaction)
}

async fn prepare_account(transaction: &mut Transaction<'_, Postgres>, user_id: &str) -> Result<AccountSnapshot, AssetError> {
    ensure_account(transaction, user_id).await?;
    expire_reservations(transaction, user_id).await?;
    account_snapshot(transaction, user_id).await
}

pub(super) async fn storage_usage(pool: &PgPool, user_id: &str) -> Result<StorageUsage, AssetError> {
    let mut transaction = pool.begin().await.map_err(storage)?;
    let account = prepare_account(&mut transaction, user_id).await?;
    transaction.commit().await.map_err(storage)?;
    Ok(StorageUsage { used_bytes: account.used.max(0) as u64, reserved_bytes: account.reserved.max(0) as u64, quota_bytes: account.limit.max(0) as u64 })
}

/// Splits the charged bytes across the libraries that hold them.
///
/// Attribution is a partition, not a per-library total: see the query for why a
/// deduplicated quota cannot be summed per library. Libraries holding nothing
/// are absent rather than reported as zero; the caller knows which libraries
/// exist and the client renders the rest.
pub(super) async fn library_storage_usage(pool: &PgPool, user_id: &str) -> Result<Vec<LibraryStorageUsage>, AssetError> {
    let rows = sqlx::query(include_str!("sql/assets/library_storage_usage.sql")).bind(user_id).fetch_all(pool).await.map_err(storage)?;
    rows.into_iter()
        .map(|row| {
            let library_id: String = row.try_get("library_id").map_err(storage)?;
            let used_bytes: i64 = row.try_get("used_bytes").map_err(storage)?;
            let library_id = library_id.parse::<LibraryId>().map_err(|error| AssetError::Storage(format!("stored library id {library_id} is not a valid identifier: {error}")))?;
            Ok(LibraryStorageUsage { library_id, used_bytes: used_bytes.max(0) as u64 })
        })
        .collect()
}

pub(super) async fn book_storage_usage(pool: &PgPool, user_id: &str, limit: usize) -> Result<Vec<BookStorageUsage>, AssetError> {
    let limit = i64::try_from(limit).map_err(storage)?;
    let rows = sqlx::query(include_str!("sql/assets/book_storage_usage.sql")).bind(user_id).bind(limit).fetch_all(pool).await.map_err(storage)?;
    rows.into_iter()
        .map(|row| {
            let hash: Vec<u8> = row.try_get("content_hash").map_err(storage)?;
            let book_id = BookId::try_from(hash.as_slice()).map_err(storage)?;
            let used_bytes: i64 = row.try_get("used_bytes").map_err(storage)?;
            Ok(BookStorageUsage { book_id, used_bytes: used_bytes.max(0) as u64 })
        })
        .collect()
}

async fn insert_blob_object(transaction: &mut Transaction<'_, Postgres>, hash: &BookId, size_bytes: i64, checksum: ContentHash) -> Result<(), AssetError> {
    let hash_str = hash.as_bytes();
    let canonical: Option<Vec<u8>> = sqlx::query_scalar("INSERT INTO canonical_blob_object(byte_checksum,size_bytes) VALUES($1,$2) ON CONFLICT(byte_checksum) DO UPDATE SET size_bytes=canonical_blob_object.size_bytes WHERE canonical_blob_object.size_bytes=excluded.size_bytes RETURNING byte_checksum")
        .bind(checksum.as_bytes().as_slice()).bind(size_bytes).fetch_optional(&mut **transaction).await.map_err(storage)?;
    if canonical.is_none() { return Err(AssetError::HashMismatch); }
    let inserted: Option<Vec<u8>> = sqlx::query_scalar(include_str!("sql/assets/insert_blob_object.sql"))
        .bind(hash_str).bind(size_bytes).bind(checksum.as_bytes().as_slice()).fetch_optional(&mut **transaction).await.map_err(storage)?;
    if inserted.is_some() {
        Ok(())
    } else {
        tracing::warn!(%hash, "stored blob size is inconsistent");
        Err(AssetError::HashMismatch)
    }
}

async fn link_canonical_revision(transaction: &mut Transaction<'_,Postgres>, store: &FsAssetStore, destination: BookId, checksum: ContentHash, size: u64) -> Result<bool,AssetError> {
    let canonical = BookId::from(checksum);
    if canonical != destination && store.link_book_revision(&canonical,&destination,checksum,size).await? { return Ok(true); }
    let sources: Vec<Vec<u8>> = sqlx::query_scalar("SELECT content_hash FROM blob_object WHERE byte_checksum=$1 AND content_hash<>$2 ORDER BY content_hash").bind(checksum.as_bytes().as_slice()).bind(destination.as_bytes()).fetch_all(&mut **transaction).await.map_err(storage)?;
    for source in sources {
        let source = BookId::try_from(source.as_slice()).map_err(storage)?;
        if store.link_book_revision(&source, &destination, checksum, size).await? { return Ok(true); }
    }
    Ok(false)
}

/// Negotiate ownership under the shared publication lock. Destructive orphan
/// repair precedes it; catalog-copy consolidation follows commit via the queue.
pub(super) async fn negotiate_books(pool: &PgPool, user_id: &str, library_id: &LibraryId, blobs: &[BlobManifestEntry], store: &FsAssetStore, mut catalog: HashMap<ContentHash, PreparedCatalogBook>) -> Result<BlobManifestResponse, AssetError> {
    let books = blobs.iter().filter(|blob| blob.size_bytes > 0).map(|blob| blob.book_id).collect::<Vec<_>>();
    repair_unbound_aliases(pool, store, &books).await?;
    let mut transaction = begin_asset_transaction(pool).await?;
    lock_publication(&mut transaction).await?;
    // Pin installation is additive. Private copies are consolidated later by
    // the collector, which alone needs the exclusive publication lock.
    let mut catalog_hashes = Vec::<Vec<u8>>::new();
    require_cloud_storage(&mut transaction, user_id, library_id).await?;
    let mut seen = HashSet::with_capacity(blobs.len());
    if blobs.iter().any(|blob| !seen.insert(blob.book_id.clone())) {
        return Err(AssetError::InvalidInput("book manifest contains a duplicate content hash".to_string()));
    }
    let mut response = BlobManifestResponse::default();
    let mut hashes = Vec::with_capacity(blobs.len());
    let mut sizes = Vec::with_capacity(blobs.len());
    let mut checksums = Vec::with_capacity(blobs.len());
    for blob in blobs {
        if blob.size_bytes == 0 {
            response.rejected.push(RejectedBlobManifestEntry { book_id: blob.book_id.clone(), reason: BlobManifestRejectionReason::InvalidSize });
            continue;
        }
        hashes.push(blob.book_id.as_bytes().to_vec());
        sizes.push(size_i64(blob.size_bytes)?);
        checksums.push(blob.byte_checksum.or_else(|| blob.book_id.content_hash()).map(|hash| hash.as_bytes().to_vec()));
    }
    if hashes.is_empty() {
        transaction.commit().await.map_err(storage)?;
        return Ok(response);
    }

    // Ensure once, then read quota only after acquiring the account lock.
    sqlx::query(include_str!("sql/account/ensure_exists.sql")).bind(user_id).execute(&mut *transaction).await.map_err(storage)?;
    expire_reservations(&mut transaction, user_id).await?;
    sqlx::query("SELECT user_id FROM user_storage_account WHERE user_id=$1 FOR UPDATE").bind(user_id).fetch_one(&mut *transaction).await.map_err(storage)?;
    let row = sqlx::query(include_str!("sql/account/select.sql")).bind(user_id).fetch_one(&mut *transaction).await.map_err(storage)?;
    let account = AccountSnapshot { limit: row.try_get("quota_bytes").map_err(storage)?, used: row.try_get("used_bytes").map_err(storage)?, reserved: row.try_get("reserved_bytes").map_err(storage)? };
    let rows = sqlx::query(include_str!("sql/assets/inspect_book_manifest.sql")).bind(&hashes).bind(&sizes).bind(user_id).bind(&checksums).fetch_all(&mut *transaction).await.map_err(storage)?;
    // Keep verified sizes after publication so every alias is validated.
    let catalog_sizes: HashMap<_, _> = catalog.iter().map(|(checksum, book)| (*checksum, book.size_bytes())).collect();
    let mut newly_owned = HashSet::new();
    let mut previewed = HashMap::new();

    let mut available = account.limit.saturating_sub(account.used).saturating_sub(account.reserved).max(0);
    for row in rows {
        let raw_hash: Vec<u8> = row.try_get("content_hash").map_err(storage)?;
        let requested_size: i64 = row.try_get("size_bytes").map_err(storage)?;
        let stored_size: Option<i64> = row.try_get("stored_size_bytes").map_err(storage)?;
        let owned: bool = row.try_get("owned").map_err(storage)?;
        let reservation_id: Option<String> = row.try_get("reservation_id").map_err(storage)?;
        let hash = BookId::try_from(raw_hash.as_slice()).map_err(storage)?;
        let requested_checksum: Option<Vec<u8>> = row.try_get("requested_checksum").map_err(storage)?;
        let requested_checksum = requested_checksum.map(|bytes| ContentHash::try_from(bytes.as_slice()).map_err(storage)).transpose()?;
        let canonical_owned: bool = row.try_get("canonical_owned").map_err(storage)?;
        let canonical_owned = canonical_owned || requested_checksum.is_some_and(|checksum| newly_owned.contains(&checksum));
        let canonical_size: Option<i64> = row.try_get("canonical_size").map_err(storage)?;
        let stored_checksum: Option<Vec<u8>> = row.try_get("stored_checksum").map_err(storage)?;
        let stored_checksum = stored_checksum.map(|bytes| ContentHash::try_from(bytes.as_slice()).map_err(storage)).transpose()?;
        if requested_checksum.and_then(|checksum| catalog_sizes.get(&checksum)).is_some_and(|size| *size != requested_size as u64) {
            response.rejected.push(RejectedBlobManifestEntry { book_id: hash, reason: BlobManifestRejectionReason::SizeMismatch });
            continue;
        }
        // Size validation must not reveal another account's stored objects.
        if owned && !hash.is_local() && requested_checksum == stored_checksum && stored_size.is_some_and(|size| size != requested_size) {
            response.rejected.push(RejectedBlobManifestEntry { book_id: hash, reason: BlobManifestRejectionReason::SizeMismatch });
            continue;
        }
        if owned {
            if store.size(user_id, AssetKind::Book, &hash).await?.is_some() {
                response.owned.push(hash);
            } else if let Some(checksum) = stored_checksum {
                let size = stored_size.ok_or(AssetError::HashMismatch)? as u64;
                if catalog.contains_key(&checksum) && requested_checksum == Some(checksum) {
                    if catalog.get_mut(&checksum).expect("checked public source").publish_and_link(store, &hash, size).await? {
                        catalog_hashes.push(checksum.as_bytes().to_vec());
                    }
                    response.owned.push(hash);
                } else if link_canonical_revision(&mut transaction,store,hash,checksum,size).await? {
                    response.owned.push(hash);
                } else { response.upload.push(hash); }
            } else { response.upload.push(hash); }
            continue;
        }
        if canonical_owned {
            let checksum = requested_checksum.ok_or(AssetError::HashMismatch)?;
            if canonical_size.is_some_and(|size| size != requested_size) {
                response.rejected.push(RejectedBlobManifestEntry { book_id: hash, reason: BlobManifestRejectionReason::SizeMismatch });
                continue;
            }
            let linked = if let Some(book) = catalog.get_mut(&checksum) {
                if book.publish_and_link(store, &hash, requested_size as u64).await? {
                    catalog_hashes.push(checksum.as_bytes().to_vec());
                }
                true
            } else { link_canonical_revision(&mut transaction,store,hash,checksum,requested_size as u64).await? };
            if linked {
                insert_blob_object(&mut transaction,&hash,requested_size,checksum).await?;
                sqlx::query(include_str!("sql/assets/insert_charge_idempotent.sql")).bind(user_id).bind(hash.as_bytes()).execute(&mut *transaction).await.map_err(storage)?;
                response.owned.push(hash);
            } else { response.upload.push(hash); }
            continue;
        }
        if reservation_id.is_some() {
            response.rejected.push(RejectedBlobManifestEntry { book_id: hash, reason: BlobManifestRejectionReason::UploadInProgress });
            continue;
        }
        // Another account's copy is never claimed by hash: knowing a hash is
        // not possessing the file. The upload proves possession; the store
        // then keeps the single existing copy.
        let preview_size = requested_checksum.and_then(|checksum| previewed.get(&checksum).copied());
        if preview_size.is_some_and(|size| size != requested_size) {
            response.rejected.push(RejectedBlobManifestEntry { book_id: hash, reason: BlobManifestRejectionReason::SizeMismatch });
        } else if preview_size.is_none() && requested_size > available {
            response.rejected.push(RejectedBlobManifestEntry { book_id: hash, reason: BlobManifestRejectionReason::QuotaExceeded });
        } else if let Some(book) = requested_checksum.and_then(|checksum| catalog.get_mut(&checksum)) {
            let checksum = requested_checksum.ok_or(AssetError::HashMismatch)?;
            insert_blob_object(&mut transaction, &hash, requested_size, checksum).await?;
            let charged = sqlx::query(include_str!("sql/assets/insert_charge_idempotent.sql")).bind(user_id).bind(hash.as_bytes()).execute(&mut *transaction).await.map_err(storage)?.rows_affected() == 1;
            if charged {
                sqlx::query("UPDATE user_storage_account SET used_bytes=used_bytes+$2 WHERE user_id=$1").bind(user_id).bind(requested_size).execute(&mut *transaction).await.map_err(storage)?;
                available -= requested_size;
            }
            if book.publish_and_link(store, &hash, requested_size as u64).await? {
                catalog_hashes.push(checksum.as_bytes().to_vec());
            }
            newly_owned.insert(checksum);
            response.owned.push(hash);
        } else {
            // This is an admission preview. The ordinary PUT reservation makes
            // the quota decision durable immediately before bytes are accepted.
            if preview_size.is_none() { available -= requested_size; }
            if let Some(checksum) = requested_checksum { previewed.insert(checksum, requested_size); }
            response.upload.push(hash);
        }
    }

    if !response.owned.is_empty() {
        let owned_hashes: Vec<_> = response.owned.iter().map(|hash| hash.as_bytes().to_vec()).collect();
        sqlx::query(include_str!("sql/cloud_storage/claim_batch.sql")).bind(library_id.to_string()).bind(user_id).bind(&owned_hashes).execute(&mut *transaction).await.map_err(storage)?;
    }
    if !catalog_hashes.is_empty() {
        sqlx::query("INSERT INTO asset_cleanup_queue(kind, content_hash) SELECT DISTINCT 'catalog', hash FROM UNNEST($1::bytea[]) AS hashes(hash)")
            .bind(&catalog_hashes).execute(&mut *transaction).await.map_err(storage)?;
    }
    transaction.commit().await.map_err(storage)?;
    Ok(response)
}

pub(super) async fn reserve_book_upload(pool: &PgPool, user_id: &str, library_id: &LibraryId, hash: &BookId, size_bytes: u64, checksum: ContentHash) -> Result<BookUploadAdmission, AssetError> {
    let size_i64 = size_i64(size_bytes)?;
    let mut transaction = begin_asset_transaction(pool).await?;
    require_cloud_storage(&mut transaction, user_id, library_id).await?;
    ensure_account(&mut transaction, user_id).await?;
    expire_reservations(&mut transaction, user_id).await?;
    let hash_str = hash.as_bytes();
    let admission = sqlx::query(include_str!("sql/assets/inspect_upload_admission.sql"))
        .bind(user_id).bind(hash_str).bind(checksum.as_bytes().as_slice()).fetch_one(&mut *transaction).await.map_err(storage)?;
    let owned: bool = admission.try_get("owned").map_err(storage)?;
    if owned {
        transaction.commit().await.map_err(storage)?;
        return Ok(BookUploadAdmission::AlreadyOwned);
    }
    let reserved: bool = admission.try_get("reserved").map_err(storage)?;
    if reserved {
        return Err(AssetError::UploadInProgress);
    }
    let reserved = sqlx::query(include_str!("sql/assets/reserve_capacity.sql")).bind(user_id).bind(size_i64).fetch_optional(&mut *transaction).await.map_err(storage)?;
    if reserved.is_none() {
        return Err(account_snapshot(&mut transaction, user_id).await?.quota_exceeded(size_bytes));
    }
    let reservation = BookUploadReservation { id: uuid::Uuid::new_v4().to_string(), user_id: user_id.to_string(), library_id: library_id.clone(), hash: hash.clone(), expected_size_bytes: size_bytes };
    let library_id_str = library_id.to_string();
    let inserted = sqlx::query(include_str!("sql/assets/insert_reservation.sql")).bind(&reservation.id).bind(user_id).bind(library_id_str).bind(hash_str).bind(size_i64).bind(UPLOAD_RESERVATION_TTL_SECONDS).execute(&mut *transaction).await;
    if let Err(error) = inserted {
        if error.as_database_error().and_then(|database| database.code()).as_deref() == Some("23505") {
            return Err(AssetError::UploadInProgress);
        }
        return Err(storage(error));
    }
    transaction.commit().await.map_err(storage)?;
    Ok(BookUploadAdmission::Reserved(reservation))
}

pub(super) async fn abort_book_upload(pool: &PgPool, reservation: &BookUploadReservation) -> Result<(), AssetError> {
    let mut transaction = pool.begin().await.map_err(storage)?;
    ensure_account(&mut transaction, &reservation.user_id).await?;
    sqlx::query(include_str!("sql/assets/abort_reservation.sql"))
        .bind(&reservation.id).bind(&reservation.user_id).execute(&mut *transaction).await.map_err(storage)?;
    transaction.commit().await.map_err(storage)
}

// Thumbnails are keyed by the book identity, not by their own bytes.
// A synced lifecycle entry is enough: the book file may still be uploading.
async fn require_thumbnail_book(transaction: &mut Transaction<'_, Postgres>, library_id: &LibraryId, hash: &BookId, size_bytes: u64) -> Result<(), AssetError> {
    if size_bytes > sync_common::MAX_THUMBNAIL_BYTES {
        return Err(AssetError::TooLarge { limit: sync_common::MAX_THUMBNAIL_BYTES });
    }
    let query = "SELECT EXISTS(SELECT 1 FROM sync_state WHERE library_id=$1 AND kind='book_lifecycle' AND version_rank<>2 AND content_hash=$2 FOR SHARE)";
    let present: bool = sqlx::query_scalar(query).bind(library_id.to_string()).bind(hash.as_bytes()).fetch_one(&mut **transaction).await.map_err(storage)?;
    if !present {
        return Err(AssetError::Forbidden);
    }
    Ok(())
}

/// Checks thumbnail access without reserving or charging user storage.
pub(super) async fn validate_thumbnail(pool: &PgPool, user_id: &str, library_id: &LibraryId, hash: &BookId, size_bytes: u64) -> Result<(), AssetError> {
    // A single snapshot checks ownership and lifecycle without a read-only
    // transaction's BEGIN/COMMIT round trips or any lifecycle row locks.
    let present: Option<bool> = sqlx::query_scalar(include_str!("sql/assets/thumbnail_read_access.sql"))
        .bind(library_id.to_string()).bind(user_id).bind(hash.as_bytes()).fetch_optional(pool).await.map_err(storage)?;
    let present = present.ok_or(AssetError::Forbidden)?;
    if size_bytes > sync_common::MAX_THUMBNAIL_BYTES {
        return Err(AssetError::TooLarge { limit: sync_common::MAX_THUMBNAIL_BYTES });
    }
    if !present {
        return Err(AssetError::Forbidden);
    }
    Ok(())
}

pub(super) async fn validate_thumbnail_upload(pool: &PgPool, user_id: &str, library_id: &LibraryId, hash: &BookId, size_bytes: u64) -> Result<(), AssetError> {
    let mut transaction = begin_asset_transaction(pool).await?;
    require_cloud_storage(&mut transaction, user_id, library_id).await?;
    require_thumbnail_book(&mut transaction, library_id, hash, size_bytes).await?;
    transaction.commit().await.map_err(storage)
}

pub(super) async fn validate_thumbnail_upload_in_transaction(transaction: &mut Transaction<'_, Postgres>, user: &str, library: &LibraryId, hash: &BookId, size: u64) -> Result<(), AssetError> {
    require_cloud_storage(transaction, user, library).await?;
    require_thumbnail_book(transaction, library, hash, size).await
}

/// Applies reference-count changes atomically with a winning lifecycle write.
/// Placement mutations are deliberately absent: a book may be present in any
/// number of directories but owns exactly one content blob reference.
pub(super) async fn apply_lifecycle_reference_deltas(transaction: &mut Transaction<'_, Postgres>, user_id: &str, deltas: impl IntoIterator<Item = (BookId, i64)>) -> Result<(), sqlx::Error> {
    let mut merged = std::collections::BTreeMap::<BookId, i64>::new();
    for (hash, delta) in deltas {
        if delta != 0 {
            *merged.entry(hash).or_default() += delta;
        }
    }
    let (hashes, values): (Vec<_>, Vec<_>) = merged.into_iter().filter(|(_, delta)| *delta != 0).map(|(hash, delta)| (hash.as_bytes().to_vec(), delta)).unzip();
    if hashes.is_empty() {
        return Ok(());
    }
    // Serialize revision replacement and reference removal for this account.
    sqlx::query("SELECT user_id FROM user_storage_account WHERE user_id=$1 FOR UPDATE").bind(user_id).fetch_optional(&mut **transaction).await?;
    let row = sqlx::query(include_str!("sql/assets/apply_blob_reference_deltas.sql")).bind(user_id).bind(&hashes).bind(&values).fetch_one(&mut **transaction).await?;
    let valid: bool = row.try_get("valid")?;
    if !valid {
        return Err(sqlx::Error::Protocol("lifecycle blob reference underflow".to_owned()));
    }
    Ok(())
}

async fn record_library_claim(tx: &mut Transaction<'_, Postgres>, user: &str, library: &LibraryId, hash: &BookId) -> Result<(), AssetError> {
    sqlx::query(include_str!("sql/cloud_storage/claim.sql")).bind(library.to_string()).bind(user).bind(hash.as_bytes()).execute(&mut **tx).await.map_err(storage)?;
    Ok(())
}

/// Charge the verified content hash and record its library claim atomically.
pub(super) async fn finish_book_upload(transaction: &mut Transaction<'_, Postgres>, user: &str, library: &LibraryId, hash: &BookId, admission: &BookUploadAdmission, actual: u64, checksum: ContentHash) -> Result<(), AssetError> {
    require_cloud_storage(transaction, user, library).await?;
    ensure_account(transaction, user).await?;
    let phase_started = Instant::now();
    sqlx::query("SELECT user_id FROM user_storage_account WHERE user_id=$1 FOR UPDATE").bind(user).fetch_one(&mut **transaction).await.map_err(storage)?;
    tracing::debug!(target: "sync_performance", phase = "upload_account_lock", elapsed_ms = phase_started.elapsed().as_secs_f64() * 1000.0);
    // Bind the logical ID after byte verification, then charge the actual
    // canonical bytes once regardless of how many logical books use them.
    insert_blob_object(transaction, hash, size_i64(actual)?, checksum).await?;
    let charged = sqlx::query(include_str!("sql/assets/insert_charge_idempotent.sql")).bind(user).bind(hash.as_bytes()).execute(&mut **transaction).await.map_err(storage)?.rows_affected() == 1;
    if let BookUploadAdmission::Reserved(reservation) = admission {
        if actual != reservation.expected_size_bytes { return Err(AssetError::LengthMismatch { expected: reservation.expected_size_bytes, actual }); }
        let released: Option<i64> = sqlx::query_scalar(include_str!("sql/assets/delete_reservation_by_id_and_user_returning_size.sql")).bind(&reservation.id).bind(user).fetch_optional(&mut **transaction).await.map_err(storage)?;
        let released = released.ok_or_else(|| AssetError::Storage("book upload reservation expired".into()))?;
        if released != size_i64(actual)? { return Err(AssetError::LengthMismatch { expected: released.max(0) as u64, actual }); }
        sqlx::query(include_str!("sql/assets/finish_upload_update_account.sql")).bind(user).bind(released).bind(charged).execute(&mut **transaction).await.map_err(storage)?;
    } else if charged {
        // Ownership may have been released while the body was streaming.
        // Recheck capacity before restoring a charge without a reservation.
        let account = account_snapshot(transaction, user).await?;
        if size_i64(actual)? > account.limit.saturating_sub(account.used).saturating_sub(account.reserved) {
            return Err(account.quota_exceeded(actual));
        }
        sqlx::query("UPDATE user_storage_account SET used_bytes=used_bytes+$2 WHERE user_id=$1").bind(user).bind(size_i64(actual)?).execute(&mut **transaction).await.map_err(storage)?;
    }
    record_library_claim(transaction, user, library, hash).await?;
    Ok(())
}
