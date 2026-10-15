//! A transactional index of author names, including writes by other curators.
use super::{author, reidentify};
use book_model::{AgentId, BookRecord};
use metadata_contract::matching::author_match_keys;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{BTreeSet, HashMap};

pub(super) fn initialize(db: &Connection) -> Result<(), String> {
    with_savepoint(db, || {
        let exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='catalog_author_credit' AND type='table')", [], |row| row.get(0)).map_err(|e| e.to_string())?;
        db.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS catalog_author_credit(
                table_name TEXT NOT NULL, record_key TEXT NOT NULL,
                position INTEGER NOT NULL, match_key TEXT NOT NULL, agent_id TEXT NOT NULL,
                PRIMARY KEY(table_name,record_key,position,match_key)
            ) STRICT, WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS catalog_author_match ON catalog_author_credit(match_key);
            CREATE TABLE IF NOT EXISTS catalog_author_dirty(
                table_name TEXT NOT NULL, record_key TEXT NOT NULL,
                PRIMARY KEY(table_name,record_key)
            ) STRICT, WITHOUT ROWID;
        ",
        )
        .map_err(|e| e.to_string())?;
        for (table, key) in TABLES {
            for (event, reference) in [("INSERT", "NEW"), ("UPDATE OF record", "NEW"), ("DELETE", "OLD")] {
                let name = event.split_whitespace().next().unwrap().to_lowercase();
                db.execute_batch(&format!("CREATE TRIGGER IF NOT EXISTS catalog_author_{table}_{name} AFTER {event} ON {table} BEGIN INSERT OR IGNORE INTO catalog_author_dirty VALUES('{table}',{reference}.{key}); END;"))
                    .map_err(|e| e.to_string())?;
            }
            if !exists {
                db.execute(&format!("INSERT OR IGNORE INTO catalog_author_dirty SELECT '{table}',{key} FROM {table}"), []).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    })
}

const TABLES: [(&str, &str); 2] = [("book", "hash"), ("external_book", "entry_key")];

// Savepoints work both inside source ingestion transactions and for standalone
// curation commands. Rolling back a book also rolls back its index and dirty flag.
fn with_savepoint<T>(db: &Connection, work: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    db.execute_batch("SAVEPOINT catalog_author_index").map_err(|e| e.to_string())?;
    match work() {
        Ok(value) => {
            if let Err(error) = db.execute_batch("RELEASE catalog_author_index") {
                let _ = db.execute_batch("ROLLBACK TO catalog_author_index; RELEASE catalog_author_index");
                return Err(error.to_string());
            }
            Ok(value)
        }
        Err(error) => {
            db.execute_batch("ROLLBACK TO catalog_author_index; RELEASE catalog_author_index").map_err(|e| e.to_string())?;
            Err(error)
        }
    }
}

fn synchronize(db: &Connection) -> Result<(), String> {
    let dirty: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM catalog_author_dirty)", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    if !dirty {
        return Ok(());
    }
    with_savepoint(db, || {
        let mut statement = db.prepare("SELECT table_name,record_key FROM catalog_author_dirty").map_err(|e| e.to_string())?;
        let pending = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).map_err(|e| e.to_string())?.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        drop(statement);
        for (table, key) in pending {
            let column = TABLES.iter().find(|(name, _)| *name == table).ok_or("unknown author index table")?.1;
            db.execute("DELETE FROM catalog_author_credit WHERE table_name=?1 AND record_key=?2", params![table, key]).map_err(|e| e.to_string())?;
            let json: Option<String> = db.query_row(&format!("SELECT record FROM {table} WHERE {column}=?1"), [&key], |row| row.get(0)).optional().map_err(|e| e.to_string())?;
            if let Some(json) = json {
                let record: BookRecord = serde_json::from_str(&json).map_err(|e| e.to_string())?;
                for (position, credit) in record.authors().enumerate() {
                    for match_key in author_match_keys(credit.name()) {
                        db.execute("INSERT OR IGNORE INTO catalog_author_credit VALUES(?1,?2,?3,?4,?5)", params![table, key, position as i64, match_key, credit.agent_id().to_string()]).map_err(|e| e.to_string())?;
                    }
                }
            }
            db.execute("DELETE FROM catalog_author_dirty WHERE table_name=?1 AND record_key=?2", params![table, key]).map_err(|e| e.to_string())?;
        }
        Ok(())
    })
}

/// Author identities by normalized name. Enriched identities win across both
/// hosted books and external links. Only changed records need deserializing.
pub struct AuthorIds(HashMap<String, AgentId>);

impl AuthorIds {
    pub fn load(db: &Connection) -> Result<Self, String> {
        let _timing = crate::timing::measure("author_index_load", "catalog");
        synchronize(db)?;
        // Preserve the previous preference: hosted records first, then external
        // records, in insertion order and author-credit order.
        let mut statement = db
            .prepare(
                "
            SELECT c.match_key,c.agent_id,0 AS kind,b.rowid AS ordinal,c.position
            FROM catalog_author_credit c JOIN book b ON c.record_key=b.hash WHERE c.table_name='book'
            UNION ALL
            SELECT c.match_key,c.agent_id,1,e.rowid,c.position
            FROM catalog_author_credit c JOIN external_book e ON c.record_key=e.entry_key WHERE c.table_name='external_book'
            ORDER BY kind,ordinal,position
        ",
            )
            .map_err(|e| e.to_string())?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).map_err(|e| e.to_string())?;
        let mut ids = HashMap::new();
        for row in rows {
            let (key, id) = row.map_err(|e| e.to_string())?;
            ids.entry(key).or_insert(AgentId::parse_str(&id).map_err(|e| e.to_string())?);
        }
        Ok(Self(ids))
    }

    pub fn assign(&mut self, record: &mut BookRecord) -> Result<(), String> {
        reidentify(record, |name, id| {
            let keys = author_match_keys(name);
            let id = keys.iter().find_map(|key| self.0.get(key)).copied().unwrap_or(id);
            for key in keys {
                self.0.entry(key).or_insert(id);
            }
            id
        })?;
        Ok(())
    }

    pub fn adopt(&mut self, db: &Connection, record: &BookRecord) -> Result<(), String> {
        let _timing = crate::timing::measure("author_identity_adopt", record.title.as_str());
        let mut adopted = HashMap::new();
        for credit in record.authors() {
            for key in author_match_keys(credit.name()) {
                adopted.insert(key, credit.agent_id());
            }
        }
        if adopted.is_empty() {
            return Ok(());
        }
        synchronize(db)?;
        let mut affected = BTreeSet::new();
        let mut statement = db.prepare("SELECT table_name,record_key FROM catalog_author_credit WHERE match_key=?1").map_err(|e| e.to_string())?;
        for key in adopted.keys() {
            let rows = statement.query_map([key], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).map_err(|e| e.to_string())?;
            for row in rows {
                affected.insert(row.map_err(|e| e.to_string())?);
            }
        }
        drop(statement);
        for (table, key) in affected {
            let column = TABLES.iter().find(|(name, _)| *name == table).ok_or("unknown author index table")?.1;
            let json: String = db.query_row(&format!("SELECT record FROM {table} WHERE {column}=?1"), [&key], |row| row.get(0)).map_err(|e| e.to_string())?;
            let mut stored: BookRecord = serde_json::from_str(&json).map_err(|e| e.to_string())?;
            let changed = reidentify(&mut stored, |name, id| author_match_keys(name).iter().find_map(|key| adopted.get(key)).copied().unwrap_or(id))?;
            if changed {
                let json = serde_json::to_string(&stored).map_err(|e| e.to_string())?;
                db.execute(&format!("UPDATE {table} SET author=?1,record=?2 WHERE {column}=?3"), params![author(&stored), json, key]).map_err(|e| e.to_string())?;
            }
        }
        self.0.extend(adopted);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curation::{self, record, set_enriched_record, set_record};
    use crate::links::{self, Entry};
    use book_model::{BookDetails, BookFormat, Contributor, AUTHOR_MARC_RELATOR_CODE, NARRATOR_MARC_RELATOR_CODE};

    fn book(name: &str, id: AgentId) -> BookRecord {
        BookRecord { metadata: BookDetails { title: "Test book".into(), contributors: vec![Contributor::with_id(id, name, AUTHOR_MARC_RELATOR_CODE).unwrap()], ..Default::default() }, description: "Original description".into() }
    }

    fn linked(db: &Connection, record: BookRecord) -> String {
        links::store_entry(
            db,
            &Entry {
                source: "test.example".into(),
                entry_id: "linked".into(),
                entry_url: "https://test.example/book".into(),
                license: "public-domain".into(),
                file_url: Some("https://test.example/audio.zip".into()),
                format: BookFormat::Mp3Folder,
                record,
                toc: Vec::new(),
                source_evidence: None,
                enriched: false,
            },
        )
        .unwrap()
    }

    #[test]
    fn adoption_reconciles_hosted_and_linked_aliases_and_preserves_other_credits() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let old = AgentId::from_u128(1);
        let canonical = AgentId::from_u128(2);
        let mut hosted = book("Verne, Jules", old);
        let narrator = Contributor::with_id(AgentId::from_u128(3), "Jules Verne", NARRATOR_MARC_RELATOR_CODE).unwrap();
        hosted.contributors.push(narrator.clone());
        set_record(&db, "hosted", &hosted).unwrap();
        let key = linked(&db, book("Jules Verne", AgentId::from_u128(4)));
        set_enriched_record(&db, "new", &book("Jules Verne", canonical)).unwrap();
        let hosted = record(&db, "hosted").unwrap().unwrap();
        assert_eq!(hosted.authors().next().unwrap().agent_id(), canonical);
        assert_eq!(hosted.contributors[1], narrator);
        assert_eq!(hosted.description, "Original description");
        let json: String = db.query_row("SELECT record FROM external_book WHERE entry_key=?1", [&key], |row| row.get(0)).unwrap();
        assert_eq!(serde_json::from_str::<BookRecord>(&json).unwrap().authors().next().unwrap().agent_id(), canonical);
        let mut next = book("Verne, Jules", old);
        AuthorIds::load(&db).unwrap().assign(&mut next).unwrap();
        assert_eq!(next.authors().next().unwrap().agent_id(), canonical);
    }

    #[test]
    fn rollback_does_not_leave_new_or_adopted_author_identities() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        let old = AgentId::from_u128(1);
        set_record(&db, "original", &book("Jane Austen", old)).unwrap();
        AuthorIds::load(&db).unwrap();
        {
            let tx = db.unchecked_transaction().unwrap();
            set_enriched_record(&tx, "new", &book("Jane Austen", AgentId::from_u128(2))).unwrap();
            set_record(&tx, "temporary", &book("Jules Verne", AgentId::from_u128(3))).unwrap();
            AuthorIds::load(&tx).unwrap(); // Synchronize inside a transaction that rolls back.
        }
        let mut next = book("Austen, Jane", AgentId::from_u128(4));
        AuthorIds::load(&db).unwrap().assign(&mut next).unwrap();
        assert_eq!(next.authors().next().unwrap().agent_id(), old);
        let indexed: i64 = db.query_row("SELECT count(*) FROM catalog_author_credit WHERE record_key IN ('new','temporary')", [], |row| row.get(0)).unwrap();
        assert_eq!(indexed, 0);
    }

    #[test]
    fn another_connections_updates_and_deletes_are_seen_without_stale_cache() {
        let dir = tempfile::tempdir().unwrap();
        let db = curation::open(dir.path()).unwrap();
        set_record(&db, "hosted", &book("Jane Austen", AgentId::from_u128(1))).unwrap();
        AuthorIds::load(&db).unwrap();
        let other = Connection::open(dir.path().join("curation.sqlite3")).unwrap();
        let changed = book("Jules Verne", AgentId::from_u128(2));
        other.execute("UPDATE book SET record=?1 WHERE hash='hosted'", [serde_json::to_string(&changed).unwrap()]).unwrap();
        let mut next = book("Verne, Jules", AgentId::from_u128(3));
        AuthorIds::load(&db).unwrap().assign(&mut next).unwrap();
        assert_eq!(next.authors().next().unwrap().agent_id(), AgentId::from_u128(2));
        other.execute("DELETE FROM book WHERE hash='hosted'", []).unwrap();
        let mut next = book("Jules Verne", AgentId::from_u128(4));
        AuthorIds::load(&db).unwrap().assign(&mut next).unwrap();
        assert_eq!(next.authors().next().unwrap().agent_id(), AgentId::from_u128(4));
        assert_eq!(db.query_row("SELECT count(*) FROM catalog_author_credit", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    }

    #[test]
    fn migration_indexes_existing_records_and_reopening_does_not_rescan_them() {
        let dir = tempfile::tempdir().unwrap();
        {
            let db = curation::open(dir.path()).unwrap();
            set_record(&db, "hosted", &book("Jane Austen", AgentId::from_u128(1))).unwrap();
            linked(&db, book("Jules Verne", AgentId::from_u128(2)));
            for (table, _) in TABLES {
                for event in ["insert", "update", "delete"] {
                    db.execute_batch(&format!("DROP TRIGGER catalog_author_{table}_{event}")).unwrap();
                }
            }
            db.execute_batch("DROP TABLE catalog_author_credit; DROP TABLE catalog_author_dirty").unwrap();
        }
        {
            let db = curation::open(dir.path()).unwrap();
            let mut next = book("Verne, Jules", AgentId::from_u128(3));
            AuthorIds::load(&db).unwrap().assign(&mut next).unwrap();
            assert_eq!(next.authors().next().unwrap().agent_id(), AgentId::from_u128(2));
        }
        let db = curation::open(dir.path()).unwrap();
        assert_eq!(db.query_row("SELECT count(*) FROM catalog_author_dirty", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    }
}
