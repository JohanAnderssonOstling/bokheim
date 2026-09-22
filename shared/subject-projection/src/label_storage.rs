//! One preferred display name and its normalized lookup key per concept.
use super::runtime::normalize_source_label;
use rusqlite::{params, Connection};

pub(super) fn upgrade(connection: &Connection) -> Result<(), String> {
    let version: String = connection.query_row("SELECT value FROM taxonomy_meta WHERE key='format_version'", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if version == "11" {
        return Ok(());
    }
    if !matches!(version.as_str(), "9" | "10") {
        return Err(format!("cannot migrate concept names from taxonomy format {version}"));
    }
    let tx = connection.unchecked_transaction().map_err(|e| e.to_string())?;
    let has_column: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('concept') WHERE name='normalized_label')", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    if !has_column {
        tx.execute_batch("ALTER TABLE concept ADD COLUMN normalized_label TEXT NOT NULL DEFAULT '';").map_err(|e| e.to_string())?;
    }
    {
        let mut select = tx.prepare("SELECT concept_id,preferred_label FROM concept").map_err(|e| e.to_string())?;
        let rows = select.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).map_err(|e| e.to_string())?;
        let mut update = tx.prepare("UPDATE concept SET normalized_label=?2 WHERE concept_id=?1").map_err(|e| e.to_string())?;
        for row in rows {
            let (id, label) = row.map_err(|e| e.to_string())?;
            update.execute(params![id, normalize_source_label(&label)]).map_err(|e| e.to_string())?;
        }
    }
    tx.execute_batch(
        "DROP TABLE IF EXISTS source_label;
        CREATE INDEX IF NOT EXISTS concept_label_lookup_idx ON concept(normalized_label,concept_id);
        UPDATE taxonomy_meta SET value='11' WHERE key='format_version';",
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_migration_preserves_aliases_and_original_schema() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE taxonomy_meta(key TEXT PRIMARY KEY,value TEXT); INSERT INTO taxonomy_meta VALUES('format_version','9'); CREATE TABLE concept(concept_id INTEGER PRIMARY KEY,preferred_label TEXT); INSERT INTO concept VALUES(1,NULL); CREATE TABLE source_label(label TEXT); INSERT INTO source_label VALUES('Original');").unwrap();
        assert!(upgrade(&db).is_err());
        assert_eq!(db.query_row("SELECT label FROM source_label", [], |r| r.get::<_, String>(0)).unwrap(), "Original");
        assert_eq!(db.query_row("SELECT value FROM taxonomy_meta", [], |r| r.get::<_, String>(0)).unwrap(), "9");
        assert_eq!(db.query_row("SELECT count(*) FROM pragma_table_info('concept') WHERE name='normalized_label'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }
    #[test]
    fn old_aliases_are_removed_and_preferred_names_are_normalized() {
        for version in ["9", "10"] {
            let db = Connection::open_in_memory().unwrap();
            db.execute_batch("CREATE TABLE taxonomy_meta(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE concept(concept_id INTEGER PRIMARY KEY,preferred_label TEXT NOT NULL); CREATE TABLE source_label(concept_id INTEGER,label TEXT); INSERT INTO source_label VALUES(1,'Alias');").unwrap();
            db.execute("INSERT INTO taxonomy_meta VALUES('format_version',?1)", [version]).unwrap();
            db.execute("INSERT INTO concept VALUES(1,?1)", ["  ÄLDRE\u{a0}Historia "]).unwrap();
            upgrade(&db).unwrap();
            upgrade(&db).unwrap();
            assert_eq!(db.query_row("SELECT preferred_label,normalized_label FROM concept", [], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap(), ("  ÄLDRE\u{a0}Historia ".into(), "äldre historia".into()));
            assert_eq!(db.query_row("SELECT count(*) FROM sqlite_master WHERE name='source_label'", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        }
    }
}
