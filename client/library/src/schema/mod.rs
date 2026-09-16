use crate::LIBRARY_SCHEMA_VERSION;
use crate::metadata::seed_subject_systems;
use crate::metadata_batch;
use crate::storage_values::parse_uuid_column;
use crate::sync_state::get_or_create_replica_id;
use crate::sync_state::queries::SyncSql;
use crate::sync_state::register_sync_functions;
use rusqlite::{Connection, Transaction, params};
use std::time::Duration;
use sync_common::ReplicaId;

use include_sqlite_sql::include_sql;

include_sql!("src/schema/sql/schema_operations.sql");

/// Every object a new library contains, in dependency order: tables before the
/// views and triggers that read them. A database is either built by this list
/// or already current, so nothing here is conditional and nothing is upgraded.
const SCHEMA: [&str; 18] = [
    include_str!("sql/authors.sql"),
    include_str!("sql/books.sql"),
    include_str!("sql/descriptions.sql"),
    include_str!("sql/jobs.sql"),
    include_str!("sql/external_metadata.sql"),
    include_str!("sql/catalog.sql"),
    include_str!("sql/placements.sql"),
    include_str!("sql/browse.sql"),
    include_str!("sql/sync.sql"),
    include_str!("sql/marks.sql"),
    include_str!("sql/assets.sql"),
    include_str!("sql/contents.sql"),
    include_str!("sql/views.sql"),
    include_str!("sql/subjects.sql"),
    include_str!("sql/sync_triggers.sql"),
    include_str!("sql/asset_triggers.sql"),
    include_str!("sql/metadata_batch.sql"),
    include_str!("sql/description_triggers.sql"),
];

pub fn configure_connection(conn: &Connection) -> rusqlite::Result<()> {
    metadata_batch::configure(conn)?;
    // Install the busy handler before any pragma that may need a database
    // lock. New pooled connections can be opened while synchronization or
    // metadata enrichment is writing on another connection.
    // Interactive reads should wait through a metadata transaction rather
    // than surface a transient "database is locked" error during scanning.
    conn.busy_timeout(Duration::from_secs(30))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let journal_mode = conn.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        conn.pragma_update(None, "journal_mode", "WAL")?;
    }
    register_sync_functions(conn)?;
    attach_curated_taxonomy(conn)
}

pub(crate) fn attach_curated_taxonomy(conn: &Connection) -> rusqlite::Result<()> {
    if conn.query_row("SELECT EXISTS(SELECT 1 FROM pragma_database_list WHERE name=?1)", ["curated"], |row| row.get::<_, bool>(0))? {
        return Ok(());
    }
    conn.attach_curated_taxonomy(":memory:")?;
    // SAFETY: this non-owning wrapper is confined to connection configuration;
    // no statement is active and it cannot outlive or close the original handle.
    // SQLite reads the static bundled bytes directly without copying the tree.
    let mut borrowed = unsafe { Connection::from_handle(conn.handle())? };
    borrowed.deserialize_bytes("curated", subject_projection::DEFAULT_UNIFIED_TAXONOMY_SQLITE)
}

pub fn library_write_transaction(conn: &mut Connection) -> rusqlite::Result<Transaction<'_>> {
    let transaction = conn.transaction()?;
    transaction.mark_library_write_transaction()?;
    Ok(transaction)
}

pub fn init_db(conn: &Connection) -> Result<ReplicaId, rusqlite::Error> {
    let version = conn.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))?;
    if version == LIBRARY_SCHEMA_VERSION {
        configure_connection(conn)?;
        let id: String = conn.query_row("SELECT value FROM sync_metadata WHERE key=?1", ["replica_id"], |row| row.get(0))?;
        return parse_uuid_column(id, 0);
    }
    if version != 0 {
        return Err(unsupported_version(version));
    }
    // Journal mode and attached databases must be configured before beginning
    // the atomic schema creation transaction.
    configure_connection(conn)?;
    // A deferred transaction writes only main. BEGIN IMMEDIATE also tries to
    // reserve the attached, read-only curated taxonomy database.
    let transaction = conn.unchecked_transaction()?;
    // Recheck inside the transaction in case another opener finished meanwhile.
    if transaction.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))? != 0 {
        transaction.rollback()?;
        return init_db(conn);
    }
    let populated: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name NOT LIKE ?1)", params!["sqlite_%"], |row| row.get(0))?;
    if populated {
        return Err(unsupported_version(version));
    }
    let replica_id = create_fresh_schema(&transaction)?;
    transaction.commit()?;
    Ok(replica_id)
}

fn unsupported_version(version: i64) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_SCHEMA), Some(format!("unsupported library schema version {version}; expected {LIBRARY_SCHEMA_VERSION}")))
}

/// The caller owns the transaction, including all seed data and version stamps.
fn create_fresh_schema(conn: &Transaction<'_>) -> rusqlite::Result<ReplicaId> {
    for objects in SCHEMA {
        conn.execute_batch(objects)?;
    }
    // Seed data defined in Rust rather than in the schema itself.
    seed_subject_systems(conn)?;
    conn.set_sync_metadata("change_origin", "local")?;
    conn.pragma_update(None, "user_version", LIBRARY_SCHEMA_VERSION)?;
    get_or_create_replica_id(conn)
}

#[cfg(test)]
mod initialization_tests;
