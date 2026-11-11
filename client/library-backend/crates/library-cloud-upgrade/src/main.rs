//! Explicit, transactional upgrade of deployed client-owned synchronization data.
use library_replica::LibraryStateEntry;
use prost::Message;
use sqlx::{PgConnection, Row, postgres::PgPoolOptions};
use std::{collections::BTreeMap, error::Error, io};
use sync_common::{ConflictRank, transport};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
mod book_id_files;
mod book_ids;

// Retain the raw protobuf Value until the current domain decoder validates it.
#[derive(Clone, PartialEq, Message)]
struct Envelope {
    #[prost(uint32, tag = "1")]
    version: u32,
    #[prost(bytes = "vec", optional, tag = "2")]
    value: Option<Vec<u8>>,
}

fn invalid(message: impl Into<String>) -> Box<dyn Error> {
    io::Error::new(io::ErrorKind::InvalidData, message.into()).into()
}

fn convert(bytes: &[u8]) -> Result<(LibraryStateEntry, Vec<u8>)> {
    if bytes.len() > transport::MAX_DECODED_REQUEST_BYTES {
        return Err(invalid("stored payload exceeds the synchronization limit"));
    }
    let mut envelope = Envelope::decode(bytes)?;
    if !(28..=transport::version()).contains(&envelope.version) || envelope.value.is_none() || envelope.encode_to_vec() != bytes {
        return Err(invalid("unsupported or non-canonical stored state envelope"));
    }
    envelope.version = transport::version();
    let value: LibraryStateEntry = transport::decode(&envelope.encode_to_vec(), transport::MAX_DECODED_REQUEST_BYTES)?;
    value.validate()?;
    let encoded = transport::encode(&value)?;
    let decoded: LibraryStateEntry = transport::decode(&encoded, transport::MAX_DECODED_REQUEST_BYTES)?;
    if value != decoded {
        return Err(invalid("state changed during current-format encoding"));
    }
    Ok((value, encoded))
}

async fn counts(connection: &mut PgConnection) -> Result<(i64, i64, i64, i64)> {
    Ok(sqlx::query_as("SELECT (SELECT count(*) FROM users),(SELECT count(*) FROM libraries),(SELECT count(*) FROM sync_state),(SELECT count(*) FROM blob_object)").fetch_one(connection).await?)
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if let [command] = arguments.as_slice() {
        if matches!(command.as_str(), "check-book-ids" | "apply-book-ids") {
            return book_ids::run(command == "apply-book-ids").await;
        }
    }
    let apply = match arguments.as_slice() {
        [command] if command == "check" => false,
        [command] if command == "apply" => true,
        _ => return Err(invalid("usage: library-cloud-upgrade check|apply|check-book-ids|apply-book-ids (DATABASE_URL required)")),
    };
    let url = std::env::var("DATABASE_URL")?;
    let pool = PgPoolOptions::new().max_connections(1).connect(&url).await?;
    let mut tx = pool.begin().await?;
    if !apply {
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY").execute(&mut *tx).await?;
    } else {
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext('bokheim-schema-upgrade'))").execute(&mut *tx).await?;
        sqlx::query("LOCK TABLE libraries,sync_state,bokheim_schema_version IN ACCESS EXCLUSIVE MODE").execute(&mut *tx).await?;
    }
    let version: i32 = sqlx::query_scalar("SELECT version FROM bokheim_schema_version").fetch_one(&mut *tx).await?;
    if !matches!(version, 14 | 17 | 18 | 19 | 20) {
        return Err(invalid(format!("unsupported database schema {version}; expected 14, 17, 18, 19 or 20")));
    }
    let before = counts(&mut tx).await?;
    let records = sqlx::query("SELECT library_id,kind,entity_key,entity_subkey,content_hash,version_rank,value FROM sync_state ORDER BY library_id,kind,entity_key,entity_subkey").fetch_all(&mut *tx).await?;
    let mut updates = Vec::new();
    let mut failures = Vec::new();
    let mut kinds = BTreeMap::<String, usize>::new();
    for row in records {
        let library: String = row.try_get("library_id")?;
        let kind: String = row.try_get("kind")?;
        let key: Vec<u8> = row.try_get("entity_key")?;
        let subkey: String = row.try_get("entity_subkey")?;
        let book: Option<Vec<u8>> = row.try_get("content_hash")?;
        let rank: i16 = row.try_get("version_rank")?;
        let bytes: Vec<u8> = row.try_get("value")?;
        match convert(&bytes) {
            Ok((value, encoded)) => {
                let address = value.address();
                let expected_rank = match value.conflict_rank() {
                    ConflictRank::Update => 0,
                    ConflictRank::Delete => 1,
                    ConflictRank::Purge => 2,
                };
                if address.kind != kind || address.entity_key.as_bytes() != key || address.entity_subkey != subkey || value.book().map(|book| book.as_bytes().to_vec()) != book || expected_rank != rank {
                    failures.push(format!("{library}/{kind}: state identity or conflict rank does not match its stored address"));
                    continue;
                }
                *kinds.entry(kind.clone()).or_default() += 1;
                if encoded != bytes {
                    updates.push((library, kind, key, subkey, encoded));
                }
            }
            Err(error) => failures.push(format!("{library}/{kind}: {error}")),
        }
    }
    if !failures.is_empty() {
        for error in failures.iter().take(20) {
            eprintln!("{error}");
        }
        return Err(invalid(format!("{} payloads cannot be migrated; no changes applied", failures.len())));
    }
    let policies = sqlx::query("SELECT id,storage_configuration FROM libraries").fetch_all(&mut *tx).await?;
    let mut external = Vec::new();
    for row in policies {
        let id: String = row.try_get("id")?;
        let raw: String = row.try_get::<serde_json::Value, _>("storage_configuration")?.to_string();
        let value: serde_json::Value = serde_json::from_str(&raw)?;
        if value.get("external").is_some() {
            external.push(id);
        } else if value != "local" && value != "bokheim" {
            return Err(invalid("unknown library storage policy; no changes applied"));
        }
    }
    println!("Validated {} records across {} state kinds; {} need encoding conversion; {} retired external-storage configurations will be archived.", before.2, kinds.len(), updates.len(), external.len());
    if apply && version == 14 {
        sqlx::raw_sql("CREATE SCHEMA IF NOT EXISTS bokheim_upgrade_archive; CREATE TABLE bokheim_upgrade_archive.sync_state_v14 AS TABLE sync_state; CREATE TABLE bokheim_upgrade_archive.library_storage_v14 AS SELECT id,storage_configuration,cloud_storage_enabled FROM libraries; ALTER TABLE sync_state DISABLE TRIGGER USER;").execute(&mut *tx).await?;
        for (library, kind, key, subkey, bytes) in &updates {
            let affected =
                sqlx::query("UPDATE sync_state SET value=$5 WHERE library_id=$1 AND kind=$2 AND entity_key=$3 AND entity_subkey=$4").bind(library).bind(kind).bind(key).bind(subkey).bind(bytes).execute(&mut *tx).await?.rows_affected();
            if affected != 1 {
                return Err(invalid("a synchronized row changed during migration"));
            }
        }
        for id in &external {
            sqlx::query("UPDATE libraries SET storage_configuration='\"local\"'::jsonb,cloud_storage_enabled=false WHERE id=$1").bind(id).execute(&mut *tx).await?;
        }
        sqlx::raw_sql("ALTER TABLE sync_state ENABLE TRIGGER USER; ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check; UPDATE bokheim_schema_version SET version=17; ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK(version=17);").execute(&mut *tx).await?;
        sqlx::query("UPDATE sync_history SET generation=$1 WHERE singleton=true").bind(uuid::Uuid::new_v4()).execute(&mut *tx).await?;
        if counts(&mut tx).await? != before {
            return Err(invalid("account, library, state, or asset counts changed during migration"));
        }
        tx.commit().await?;
        println!("Committed schema 14 → 17; accounts, libraries, state versions and assets preserved. Original payloads and storage policies remain in bokheim_upgrade_archive; legacy feeds remain in opds_feeds.");
    } else {
        // The book-ID upgrade rewrites supported earlier envelopes together
        // with their addresses after the billing schema reaches version 19.
        if apply && ((!updates.is_empty() && version == 20) || !external.is_empty()) {
            return Err(invalid(format!("schema {version} still contains unmigrated data")));
        }
        tx.rollback().await?;
        println!("{} completed without changes.", if apply { "Already-current migration" } else { "Read-only preflight" });
    }
    Ok(())
}
