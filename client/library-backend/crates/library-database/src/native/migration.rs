//! Upgrade path-addressed materialization work to stable placement identities.
use super::NativeFileWorkSql;
use crate::placements::{FolderPaths, stored_book_path};
use crate::query_methods::query_rows;
use crate::{DatabaseError, native::TablesSql};
use rusqlite::Transaction;
use sync_common::BookId;

pub(crate) fn migrate_file_work(tx: &Transaction<'_>) -> Result<(), DatabaseError> {
    let already_upgraded: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('local_file_work') WHERE name='dir_id')", [], |row| row.get(0))?;
    if already_upgraded {
        tx.schema_native_tables()?;
        return Ok(());
    }
    tx.execute_batch("ALTER TABLE local_file_work RENAME TO legacy_file_work")?;
    tx.schema_native_tables()?;
    tx.execute_batch(
        "INSERT INTO local_file_work(id,operation,content_hash,relative_path)
         SELECT id,operation,content_hash,relative_path FROM legacy_file_work WHERE operation='remove';
         INSERT INTO sqlite_sequence(name,seq) SELECT 'local_file_work',0 WHERE NOT EXISTS(SELECT 1 FROM sqlite_sequence WHERE name='local_file_work');
         UPDATE sqlite_sequence SET seq=MAX(seq,COALESCE((SELECT MAX(id) FROM legacy_file_work),0)) WHERE name='local_file_work';",
    )?;
    let jobs = tx
        .prepare("SELECT id,content_hash,relative_path FROM legacy_file_work WHERE operation='materialize' ORDER BY id")?
        .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, BookId>(1)?, row.get::<_, String>(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut folders = if jobs.len() > 1 { FolderPaths::all(tx)? } else { FolderPaths::new(tx) };
    for (id, book, path) in jobs {
        let path = stored_book_path(&path)?;
        let placements = query_rows(|read| tx.native_live_placement_paths(&book, read), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        let projections = query_rows(|read| tx.native_file_projection_paths(&book, read), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        let mut matched = Vec::new();
        for (directory, name) in &placements {
            if folders.placement(directory, name)?.as_ref() == Some(&path) || projections.iter().any(|(dir, previous)| dir == directory && previous == path.as_str()) {
                matched.push(directory);
            }
        }
        // Older work may already have lost its path during an unobserved rename.
        // Preserve that request for the book's live placements rather than lose it.
        if matched.is_empty() {
            matched.extend(placements.iter().map(|(directory, _)| directory));
        }
        for (index, directory) in matched.into_iter().enumerate() {
            tx.execute("INSERT OR IGNORE INTO local_file_work(id,operation,content_hash,dir_id) VALUES(?1,'materialize',?2,?3)", (if index == 0 { Some(id) } else { None }, book, directory))?;
        }
    }
    tx.execute_batch("DROP TABLE legacy_file_work")?;
    Ok(())
}

/// Older historical cleanup has no proof of ownership. Keep it unbound so
/// replay can retire the obsolete intent without deleting another store's files.
pub(crate) fn migrate_cleanup_owners(tx: &Transaction<'_>) -> Result<(), DatabaseError> {
    for (table, column, definition) in [
        ("local_file_work", "source_identity", "TEXT"),
        ("local_file_work", "source_library", "TEXT"),
        ("local_storage_move", "source_identity", "TEXT"),
        ("local_storage_move", "source_library", "TEXT"),
        ("local_storage_move", "adopted_json", "TEXT NOT NULL DEFAULT '[]'"),
    ] {
        let present: bool = tx.query_row(&format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name=?1)"), [column], |row| row.get(0))?;
        if !present {
            tx.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"))?;
        }
    }
    tx.execute_batch(
        "DROP INDEX IF EXISTS idx_local_file_cleanup;
        CREATE UNIQUE INDEX idx_local_file_cleanup ON local_file_work(content_hash,relative_path,COALESCE(source_root,''),COALESCE(source_identity,''),COALESCE(source_library,'')) WHERE operation='remove';",
    )?;
    Ok(())
}
