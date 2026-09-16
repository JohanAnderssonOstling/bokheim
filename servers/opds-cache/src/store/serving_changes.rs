//! Transactional, coalescing invalidation for the independent browse projector.
//!
//! A publication stays dirty until the projector acknowledges the exact
//! sequence it read. A concurrent edit therefore cannot be acknowledged away.
use super::*;

pub(super) fn install(connection: &Connection) -> Result<(), CatalogStoreError> {
    let installed: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='catalog_serving_dirty')", [], |row| row.get(0))?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS catalog_serving_change_sequence (
             singleton INTEGER PRIMARY KEY CHECK(singleton=1), sequence INTEGER NOT NULL
         );
         INSERT OR IGNORE INTO catalog_serving_change_sequence VALUES(1,0);
         CREATE TABLE IF NOT EXISTS catalog_serving_dirty (
             source_id TEXT NOT NULL, edition_id TEXT NOT NULL, work_id TEXT NOT NULL,
             sequence INTEGER NOT NULL,
             PRIMARY KEY(source_id,edition_id,work_id)
         ) WITHOUT ROWID;
         CREATE INDEX IF NOT EXISTS catalog_serving_dirty_sequence ON catalog_serving_dirty(sequence);
         CREATE TABLE IF NOT EXISTS catalog_serving_dirty_source (
             source_id TEXT PRIMARY KEY, sequence INTEGER NOT NULL
         ) WITHOUT ROWID;",
    )?;
    if !installed {
        connection.execute_batch(
            "UPDATE catalog_serving_change_sequence SET sequence=sequence+1 WHERE singleton=1;
             INSERT INTO catalog_serving_dirty_source(source_id,sequence)
             SELECT source_id,(SELECT sequence FROM catalog_serving_change_sequence WHERE singleton=1)
             FROM catalog_source;",
        )?;
    }
    // Track all inputs, including edits to normalized child tables that do
    // not update the serialized publication payload. Keep OLD keys on UPDATE
    // as well, so identity moves remove the previous serving entry.
    for table in ["catalog_publication", "catalog_publication_author", "catalog_publication_toc", "catalog_publication_language", "catalog_publication_format", "catalog_publication_popularity", "catalog_publication_subject_assignment"] {
        for (event, references) in [("INSERT", &["NEW"][..]), ("UPDATE", &["OLD", "NEW"][..]), ("DELETE", &["OLD"][..])] {
            // The projector advances this epoch while pinning its read snapshot.
            // Repeated child writes before that fence only dirty a book once.
            let mut body = String::new();
            for reference in references {
                let moved = if event == "UPDATE" && *reference == "OLD" { " AND (OLD.source_id IS NOT NEW.source_id OR OLD.edition_id IS NOT NEW.edition_id OR OLD.work_id IS NOT NEW.work_id OR OLD.generation IS NOT NEW.generation)" } else { "" };
                body.push_str(&format!(
                    "INSERT INTO catalog_serving_dirty(source_id,edition_id,work_id,sequence)
                     SELECT {reference}.source_id,{reference}.edition_id,{reference}.work_id,
                            (SELECT sequence FROM catalog_serving_change_sequence WHERE singleton=1)
                     WHERE EXISTS(SELECT 1 FROM catalog_source WHERE source_id={reference}.source_id AND active_generation={reference}.generation){moved}
                     ON CONFLICT(source_id,edition_id,work_id) DO UPDATE SET sequence=excluded.sequence
                     WHERE catalog_serving_dirty.sequence!=excluded.sequence;"
                ));
            }
            connection.execute_batch(&format!(
                "DROP TRIGGER IF EXISTS serving_dirty_{table}_{event};
                 CREATE TRIGGER serving_dirty_{table}_{event} AFTER {event} ON {table} BEGIN {body} END;"
            ))?;
        }
    }
    // Membership rows are derived from assignments by the projector itself.
    for event in ["INSERT", "UPDATE", "DELETE"] {
        connection.execute_batch(&format!("DROP TRIGGER IF EXISTS serving_dirty_catalog_publication_subject_node_{event}"))?;
    }
    // Generation activation changes visibility without changing publication
    // rows. Record a source marker in O(1); the projector expands it outside
    // the authoritative writer, including deletions from its previous view.
    for (event, reference, condition) in [("INSERT", "NEW", ""), ("UPDATE", "NEW", "WHEN OLD.active_generation IS NOT NEW.active_generation"), ("DELETE", "OLD", "")] {
        connection.execute_batch(&format!(
            "DROP TRIGGER IF EXISTS serving_dirty_source_{event};
             CREATE TRIGGER serving_dirty_source_{event} AFTER {event} ON catalog_source {condition}
             BEGIN
               INSERT INTO catalog_serving_dirty_source(source_id,sequence)
               VALUES({reference}.source_id,(SELECT sequence FROM catalog_serving_change_sequence WHERE singleton=1))
               ON CONFLICT(source_id) DO UPDATE SET sequence=excluded.sequence
               WHERE catalog_serving_dirty_source.sequence!=excluded.sequence;
             END;"
        ))?;
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub(super) struct PublicationChange {
    pub source_id: String,
    pub edition_id: String,
    pub work_id: String,
    pub sequence: i64,
}

#[derive(Clone, Debug)]
pub(super) struct SourceChange {
    pub source_id: String,
    pub sequence: i64,
}

/// Called within the same read snapshot used to prepare projected rows.
pub(super) fn publications(connection: &Connection, limit: usize) -> Result<Vec<PublicationChange>, CatalogStoreError> {
    let mut statement = connection.prepare("SELECT source_id,edition_id,work_id,sequence FROM catalog_serving_dirty ORDER BY sequence LIMIT ?1")?;
    let rows = statement.query_map([limit.clamp(1, 512) as i64], |row| Ok(PublicationChange { source_id: row.get(0)?, edition_id: row.get(1)?, work_id: row.get(2)?, sequence: row.get(3)? }))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub(super) fn sources(connection: &Connection) -> Result<Vec<SourceChange>, CatalogStoreError> {
    let mut statement = connection.prepare("SELECT source_id,sequence FROM catalog_serving_dirty_source ORDER BY sequence LIMIT 16")?;
    let rows = statement.query_map([], |row| Ok(SourceChange { source_id: row.get(0)?, sequence: row.get(1)? }))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Only acknowledge after the serving transaction commits. Replaying after a
/// crash between that commit and this acknowledgement is intentionally safe.
pub(super) fn acknowledge(connection: &Connection, publications: &[PublicationChange], sources: &[SourceChange]) -> Result<(), CatalogStoreError> {
    for change in publications {
        connection.execute("DELETE FROM catalog_serving_dirty WHERE source_id=?1 AND edition_id=?2 AND work_id=?3 AND sequence=?4", params![change.source_id, change.edition_id, change.work_id, change.sequence])?;
    }
    for change in sources {
        connection.execute("DELETE FROM catalog_serving_dirty_source WHERE source_id=?1 AND sequence=?2", params![change.source_id, change.sequence])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_fence_preserves_concurrent_edits_and_coalesces_child_writes() {
        let directory = tempfile::tempdir().unwrap();
        let store = CatalogStore::open(directory.path().join("catalog.db")).unwrap();
        let mut writer = store.connection().unwrap();
        writer.pragma_update(None, "foreign_keys", "OFF").unwrap();
        writer.execute("INSERT INTO catalog_source(source_id,source_url,epoch,active_generation,next_sequence) VALUES('source','url','epoch',1,1)", []).unwrap();
        writer.execute_batch("CREATE TABLE test_dirty_writes(n INTEGER); CREATE TRIGGER test_dirty_update AFTER UPDATE ON catalog_serving_dirty BEGIN INSERT INTO test_dirty_writes VALUES(1); END;").unwrap();
        writer.execute("INSERT INTO catalog_publication_author VALUES('source',1,'edition','',0,'before')", []).unwrap();
        for _ in 0..100 {
            writer.execute("UPDATE catalog_publication_author SET normalized_name='before'", []).unwrap();
        }
        assert_eq!(writer.query_row("SELECT COUNT(*) FROM test_dirty_writes", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        let mut reader = store.connection().unwrap();
        let fence = writer.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
        let snapshot = reader.transaction().unwrap();
        let batch = publications(&snapshot, 256).unwrap();
        fence.execute("UPDATE catalog_serving_change_sequence SET sequence=sequence+1", []).unwrap();
        fence.commit().unwrap();
        // Preparation keeps a WAL read snapshot, not a writer reservation.
        writer.execute("UPDATE catalog_publication_author SET normalized_name='after'", []).unwrap();
        assert_eq!(snapshot.query_row("SELECT normalized_name FROM catalog_publication_author", [], |r| r.get::<_, String>(0)).unwrap(), "before");
        snapshot.commit().unwrap();
        acknowledge(&writer, &batch, &[]).unwrap();
        assert_eq!(publications(&writer, 256).unwrap().len(), 1);
        assert_eq!(writer.query_row("SELECT COUNT(*) FROM test_dirty_writes", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        writer.execute("INSERT INTO catalog_publication_author VALUES('source',2,'inactive','',0,'staged')", []).unwrap();
        assert_eq!(publications(&writer, 256).unwrap().len(), 1, "inactive staging must use the activation marker");
        acknowledge(&writer, &publications(&writer, 256).unwrap(), &[]).unwrap();
        writer.execute("UPDATE catalog_publication_author SET generation=2 WHERE edition_id='edition'", []).unwrap();
        assert_eq!(publications(&writer,256).unwrap().len(),1,"moving out of the active generation must invalidate the old key");
    }

    #[test]
    fn changes_coalesce_rollback_and_preserve_newer_edits() {
        let directory = tempfile::tempdir().unwrap();
        let store = CatalogStore::open(directory.path().join("catalog.db")).unwrap();
        let mut connection = store.connection().unwrap();
        // Child inputs deliberately use a small isolated table fixture: the
        // trigger itself must behave identically inside a caller transaction.
        connection.pragma_update(None, "foreign_keys", "OFF").unwrap();
        connection.execute("INSERT INTO catalog_source(source_id,source_url,epoch,active_generation,next_sequence) VALUES('source','url','epoch',1,1)", []).unwrap();
        let insert = "INSERT INTO catalog_publication_author(source_id,generation,edition_id,work_id,position,normalized_name)
                      VALUES('source',1,'edition','',0,'first')";
        {
            let transaction = connection.transaction().unwrap();
            transaction.execute(insert, []).unwrap();
            transaction.rollback().unwrap();
        }
        assert_eq!(connection.query_row("SELECT COUNT(*) FROM catalog_serving_dirty", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        connection.execute(insert, []).unwrap();
        let batch = publications(&connection, 32).unwrap();
        let observed: i64 = connection.query_row("SELECT sequence FROM catalog_serving_dirty", [], |r| r.get(0)).unwrap();
        connection.execute("UPDATE catalog_serving_change_sequence SET sequence=sequence+1", []).unwrap();
        connection.execute("UPDATE catalog_publication_author SET normalized_name='second'", []).unwrap();
        acknowledge(&connection, &batch, &[]).unwrap();
        assert_eq!(connection.execute("DELETE FROM catalog_serving_dirty WHERE sequence=?1", [observed]).unwrap(), 0);
        assert_eq!(connection.query_row("SELECT COUNT(*) FROM catalog_serving_dirty", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
        connection.execute("DELETE FROM catalog_publication_author", []).unwrap();
        assert!(connection.query_row("SELECT sequence FROM catalog_serving_dirty", [], |r| r.get::<_, i64>(0)).unwrap() > observed);
        let batch = publications(&connection, 32).unwrap();
        acknowledge(&connection, &batch, &[]).unwrap();
        assert!(publications(&connection, 32).unwrap().is_empty());
    }

    #[test]
    fn activation_and_removal_survive_restart_until_acknowledged() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("catalog.db");
        let store = CatalogStore::open(&path).unwrap();
        let source = CatalogSourceId::parse("opds-fixture").unwrap();
        store.ensure_source(&source, "https://example.com/catalog").unwrap();
        let connection = store.connection().unwrap();
        let first = sources(&connection).unwrap();
        connection.execute("UPDATE catalog_serving_change_sequence SET sequence=sequence+1", []).unwrap();
        assert_eq!(first.len(), 1);
        connection.execute("UPDATE catalog_source SET active_generation=2 WHERE source_id=?1", [source.as_str()]).unwrap();
        acknowledge(&connection, &[], &first).unwrap();
        assert_eq!(sources(&connection).unwrap().len(), 1);
        drop(connection);
        drop(store);
        let store = CatalogStore::open(&path).unwrap();
        let connection = store.connection().unwrap();
        let activated = sources(&connection).unwrap();
        assert_eq!(activated.len(), 1);
        acknowledge(&connection, &[], &activated).unwrap();
        connection.execute("DELETE FROM catalog_source WHERE source_id=?1", [source.as_str()]).unwrap();
        assert_eq!(sources(&connection).unwrap().len(), 1);
    }
}
