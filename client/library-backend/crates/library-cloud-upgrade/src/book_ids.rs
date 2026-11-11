//! Explicit migration of client-owned identities, including opaque state bodies.
use super::{Result, convert, invalid};
use content_address::ContentHash;
use library_replica::{BookLifecycleState, LibraryStateEntry};
use sqlx::{Row, postgres::PgPoolOptions};
use std::collections::{BTreeMap, BTreeSet};
use sync_common::{BookId, transport};

pub(super) async fn run(apply: bool) -> Result<()> {
    let pool = PgPoolOptions::new().max_connections(1).connect(&std::env::var("DATABASE_URL")?).await?;
    let mut tx = pool.begin().await?;
    if apply {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('bokheim-schema-upgrade'))").execute(&mut *tx).await?;
        sqlx::raw_sql("LOCK TABLE bokheim_schema_version,libraries,sync_state,blob_object,library_blob_claim,user_blob_charge,user_blob_reference,user_storage_account,book_upload_reservation IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY").execute(&mut *tx).await?;
    }
    let version: i32 = sqlx::query_scalar("SELECT version FROM bokheim_schema_version").fetch_one(&mut *tx).await?;
    if version == 20 {
        println!("Global book identities are already installed.");
        tx.rollback().await?;
        return Ok(());
    }
    if !apply && matches!(version, 14 | 17 | 18) {
        println!("Earlier schema upgrades must run before the book-ID preflight.");
        tx.rollback().await?;
        return Ok(());
    }
    if version != 19 {
        return Err(invalid("book identity migration requires schema 19; run the earlier schema upgrades first"));
    }
    let blobs = sqlx::query("SELECT content_hash,byte_checksum,size_bytes FROM blob_object").fetch_all(&mut *tx).await?;
    let mut files = Vec::new();
    let mut ids = BTreeMap::<BookId, BookId>::new();
    let mut checksums = BTreeMap::<BookId, ContentHash>::new();
    for row in blobs {
        let old = BookId::try_from(row.try_get::<Vec<u8>, _>("content_hash")?.as_slice())?;
        let checksum = ContentHash::try_from(row.try_get::<Vec<u8>, _>("byte_checksum")?.as_slice())?;
        files.push((old, checksum, u64::try_from(row.try_get::<i64, _>("size_bytes")?)?));
        checksums.insert(old, checksum);
        if old.is_library_scoped() {
            ids.insert(old, BookId::for_local_content(checksum));
        }
    }
    let rows = sqlx::query("SELECT library_id,kind,entity_key,entity_subkey,content_hash,value,changed_at,version_rank,replica_id,replica_seq,server_seq FROM sync_state ORDER BY server_seq").fetch_all(&mut *tx).await?;
    let mut entries = Vec::with_capacity(rows.len());
    let mut purged = BTreeSet::new();
    for row in rows {
        let (value, _) = convert(&row.try_get::<Vec<u8>, _>("value")?)?;
        let address = value.address();
        if address.kind != row.try_get::<String, _>("kind")?
            || address.entity_key.as_bytes() != row.try_get::<Vec<u8>, _>("entity_key")?
            || address.entity_subkey != row.try_get::<String, _>("entity_subkey")?
            || value.book().map(|book| book.storage_bytes()) != row.try_get::<Option<Vec<u8>>, _>("content_hash")?
            || i16::from(u8::from(value.conflict_rank())) != row.try_get::<i16, _>("version_rank")?
        {
            return Err(invalid("stored state body does not match its address"));
        }
        if let LibraryStateEntry::BookContent { book_id, checksum, .. } = &value {
            if checksums.insert(*book_id, *checksum).is_some_and(|known| known != *checksum) {
                return Err(invalid("book content disagrees with its uploaded checksum"));
            }
            if book_id.is_library_scoped() {
                ids.insert(*book_id, BookId::for_local_content(*checksum));
            }
        }
        if let LibraryStateEntry::BookLifecycle { book_id, value: BookLifecycleState::Purged } = &value {
            purged.insert((row.try_get::<String, _>("library_id")?, *book_id));
        }
        entries.push((row, value));
    }
    for (row, value) in &entries {
        if let Some(book) = value.book().filter(|book| book.is_library_scoped() && !ids.contains_key(book)) {
            if !purged.contains(&(row.try_get::<String, _>("library_id")?, book)) {
                return Err(invalid(format!("cannot migrate live book {book}: no uploaded content checksum is available")));
            }
        }
    }
    if ids.values().any(|new| ids.get(new).is_some_and(|mapped| mapped != new)) {
        return Err(invalid("overlapping old and new book identities"));
    }
    println!("Validated {} synchronized records; {} scoped book identities will be replaced. Blob bytes and canonical quota charges remain unchanged.", entries.len(), ids.len());
    super::book_id_files::prepare(&ids, &files, false)?;
    if !apply {
        tx.rollback().await?;
        return Ok(());
    }
    // Keep an exact, transactional rollback source before interpreting any
    // domain values. This operation is run with service traffic stopped.
    sqlx::raw_sql(
        "CREATE SCHEMA IF NOT EXISTS bokheim_upgrade_archive;
         CREATE TABLE bokheim_upgrade_archive.sync_state_v19 AS TABLE sync_state;
         CREATE TABLE bokheim_upgrade_archive.blob_object_v19 AS TABLE blob_object;
         CREATE TABLE bokheim_upgrade_archive.library_blob_claim_v19 AS TABLE library_blob_claim;
         CREATE TABLE bokheim_upgrade_archive.user_blob_charge_v19 AS TABLE user_blob_charge;
         CREATE TABLE bokheim_upgrade_archive.user_blob_reference_v19 AS TABLE user_blob_reference;
         CREATE TABLE bokheim_upgrade_archive.book_upload_reservation_v19 AS TABLE book_upload_reservation;
         CREATE TABLE bokheim_upgrade_archive.user_storage_account_v19 AS TABLE user_storage_account;
         CREATE TEMP TABLE book_id_rekey(old_id bytea PRIMARY KEY,new_id bytea NOT NULL) ON COMMIT DROP;
         CREATE TEMP TABLE rekeyed_state (LIKE sync_state INCLUDING DEFAULTS) ON COMMIT DROP;
         ALTER TABLE rekeyed_state ADD PRIMARY KEY(library_id,kind,entity_key,entity_subkey);",
    )
    .execute(&mut *tx)
    .await?;
    for (old, new) in &ids {
        sqlx::query("INSERT INTO book_id_rekey VALUES($1,$2)").bind(old.as_bytes()).bind(new.as_bytes()).execute(&mut *tx).await?;
    }
    for (row, mut value) in entries {
        value.remap_books(|book| ids.get(&book).copied().unwrap_or(book));
        value.validate()?;
        let address = value.address();
        // Rows arrive by original acceptance sequence. Keep the first book
        // addition and the greatest authored version of every mutable cell.
        sqlx::query(
            "INSERT INTO rekeyed_state(library_id,kind,entity_key,entity_subkey,content_hash,value,changed_at,version_rank,replica_id,replica_seq,server_seq)
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
             ON CONFLICT(library_id,kind,entity_key,entity_subkey) DO UPDATE SET content_hash=excluded.content_hash,value=excluded.value,changed_at=excluded.changed_at,version_rank=excluded.version_rank,replica_id=excluded.replica_id,replica_seq=excluded.replica_seq,server_seq=excluded.server_seq
             WHERE excluded.kind<>'book_add' AND (excluded.changed_at,excluded.version_rank,excluded.replica_id COLLATE \"C\",excluded.replica_seq)>(rekeyed_state.changed_at,rekeyed_state.version_rank,rekeyed_state.replica_id COLLATE \"C\",rekeyed_state.replica_seq)"
        ).bind(row.try_get::<String, _>("library_id")?).bind(address.kind).bind(address.entity_key.as_bytes()).bind(address.entity_subkey)
            .bind(value.book().map(|book| book.storage_bytes())).bind(transport::encode(&value)?)
            .bind(row.try_get::<i64, _>("changed_at")?).bind(row.try_get::<i16, _>("version_rank")?).bind(row.try_get::<String, _>("replica_id")?)
            .bind(row.try_get::<i64, _>("replica_seq")?).bind(row.try_get::<i64, _>("server_seq")?).execute(&mut *tx).await?;
    }
    let conflicts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (SELECT COALESCE(m.new_id,b.content_hash) FROM blob_object b LEFT JOIN book_id_rekey m ON m.old_id=b.content_hash GROUP BY 1 HAVING count(DISTINCT byte_checksum)>1 OR count(DISTINCT size_bytes)>1) conflicting",
    )
    .fetch_one(&mut *tx)
    .await?;
    if conflicts != 0 {
        return Err(invalid("global identity would combine different uploaded revisions"));
    }
    super::book_id_files::prepare(&ids, &files, true)?;
    sqlx::raw_sql(include_str!("book_ids.sql")).execute(&mut *tx).await?;
    tx.commit().await?;
    println!("Committed schema 19 → 20. State, asset references and embedded IDs migrated; sync history rotated. Original records remain in bokheim_upgrade_archive.");
    Ok(())
}
