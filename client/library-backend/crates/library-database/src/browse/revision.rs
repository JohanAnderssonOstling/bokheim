//! Transactional dependency counters shared by all connections to a library.
//! Background queues, cover bookkeeping and no-op updates do not expire browse results.
use crate::DatabaseError;
use rusqlite::Connection;

pub(crate) fn taxonomy(connection: &Connection) -> Result<Option<String>, DatabaseError> {
    let installed: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM curated.sqlite_schema WHERE name='taxonomy_meta' AND type='table')", [], |row| row.get(0))?;
    if installed { crate::taxonomy_update::revision(connection).map(Some) } else { Ok(None) }
}

#[cfg(test)]
pub(crate) fn remove(connection: &Connection) -> Result<(), DatabaseError> {
    let triggers = connection.prepare("SELECT name FROM sqlite_schema WHERE type='trigger' AND name GLOB 'browse_*'")?.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    for trigger in triggers {
        connection.execute_batch(&format!("DROP TRIGGER {trigger}"))?;
    }
    connection.execute_batch("DROP TABLE browse_revision")?;
    Ok(())
}

#[derive(Clone, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Revision {
    pub visibility: i64,
    pub subjects: i64,
    pub authors: i64,
    pub metadata: i64,
    pub availability: i64,
    pub reading: i64,
    pub folders: i64,
    pub facets: i64,
    // Older, uninitialized databases retain conservative invalidation.
    pub legacy: Option<(u64, i64)>,
}

impl Revision {
    pub(crate) fn read(connection: &Connection) -> Result<Self, DatabaseError> {
        let installed: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM main.sqlite_schema WHERE name='browse_revision' AND type='table')", [], |row| row.get(0))?;
        if !installed {
            return Ok(Self { legacy: Some((connection.total_changes(), connection.pragma_query_value(None, "data_version", |row| row.get(0))?)), ..Default::default() });
        }
        Ok(connection.query_row("SELECT visibility,subjects,authors,metadata,availability,reading,folders,facets FROM browse_revision WHERE singleton=1", [], |row| {
            Ok(Self { visibility: row.get(0)?, subjects: row.get(1)?, authors: row.get(2)?, metadata: row.get(3)?, availability: row.get(4)?, reading: row.get(5)?, folders: row.get(6)?, facets: row.get(7)?, legacy: None })
        })?)
    }
}

/// Installed in the schema transaction, so revisions and source writes commit
/// or roll back together, including writes made through other connections.
pub(crate) fn install(connection: &Connection) -> Result<(), DatabaseError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS browse_revision(
        singleton INTEGER PRIMARY KEY CHECK(singleton=1),
        visibility INTEGER NOT NULL DEFAULT 0,subjects INTEGER NOT NULL DEFAULT 0,
        authors INTEGER NOT NULL DEFAULT 0,metadata INTEGER NOT NULL DEFAULT 0,
        availability INTEGER NOT NULL DEFAULT 0,reading INTEGER NOT NULL DEFAULT 0,
        folders INTEGER NOT NULL DEFAULT 0,facets INTEGER NOT NULL DEFAULT 0
    ) STRICT;
    INSERT OR IGNORE INTO browse_revision(singleton) VALUES(1);",
    )?;
    // Each tuple describes one independently observable change. Update guards
    // inspect values, rather than merely the columns mentioned by a writer.
    for (name, table, domains, columns, rows) in [
        ("book_visibility", "book", "visibility,facets", "content_hash,deleted_at", true),
        ("book_metadata", "book", "metadata", "content_hash,title,subtitle,author,search_title,search_author,format,popularity_rating_count,duration_ms", true),
        ("book_reading", "book", "reading", "read_progress,read_at,added_at", true),
        ("book_format", "book", "facets", "format", false),
        ("subjects", "book_unified_concept", "subjects", "book_row_id,concept_id", true),
        ("credits", "book_contributor", "authors,metadata", "book_row_id,author_identity_id,name,role", true),
        ("authors", "author_identity", "authors", "id,stable_id", true),
        ("languages", "book_language", "metadata,facets", "book_row_id,language", true),
        ("placement_visibility", "book_dir", "visibility,facets", "book_row_id,deleted_at", true),
        ("placement_availability", "book_dir", "availability", "book_row_id,deleted_at,is_downloaded", true),
        ("placements", "book_dir", "folders", "book_row_id,dir_id,file_name,deleted_at", true),
        ("files", "local_file_projection", "availability", "content_hash", true),
        ("folders", "dir", "folders", "id,parent_id,name,deleted_at", true),
        ("download_request", "book_transfer", "availability", "book_download", false),
    ] {
        let assignments = domains.split(',').map(|domain| format!("{domain}={domain}+1")).collect::<Vec<_>>().join(",");
        let update = format!("UPDATE browse_revision SET {assignments} WHERE singleton=1;");
        let changed = columns.split(',').map(|column| format!("OLD.{column} IS NOT NEW.{column}")).collect::<Vec<_>>().join(" OR ");
        connection.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS browse_{name}_update AFTER UPDATE OF {columns} ON {table} WHEN {changed} BEGIN {update} END;"))?;
        if rows {
            for event in ["INSERT", "DELETE"] {
                connection.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS browse_{name}_{} AFTER {event} ON {table} BEGIN {update} END;", event.to_ascii_lowercase()))?;
            }
        }
    }
    for (event, record) in [("INSERT", "NEW"), ("DELETE", "OLD")] {
        connection.execute_batch(&format!(
            "CREATE TRIGGER IF NOT EXISTS browse_download_request_{} AFTER {event} ON book_transfer WHEN {record}.book_download IS NOT NULL BEGIN UPDATE browse_revision SET availability=availability+1 WHERE singleton=1; END;",
            event.to_ascii_lowercase()
        ))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    #[test]
    fn version_40_migration_installs_transactional_counters_once() {
        let db = Database::open(":memory:").unwrap();
        let replica = db.initialize_library().unwrap();
        remove(&db.connection).unwrap();
        db.connection.pragma_update(None, "user_version", 40).unwrap();
        assert!(Revision::read(&db.connection).unwrap().legacy.is_some());
        assert_eq!(db.initialize_library().unwrap(), replica);
        let before = Revision::read(&db.connection).unwrap();
        assert!(before.legacy.is_none());
        let transaction = db.connection.unchecked_transaction().unwrap();
        transaction.execute("UPDATE dir SET name='temporary'", []).unwrap();
        assert!(Revision::read(&transaction).unwrap().folders > before.folders);
        transaction.rollback().unwrap();
        assert!(Revision::read(&db.connection).unwrap() == before);
        assert_eq!(db.initialize_library().unwrap(), replica);
        assert!(Revision::read(&db.connection).unwrap() == before);
        db.check_migrated().unwrap();
    }
}
