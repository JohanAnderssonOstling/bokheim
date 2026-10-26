use super::*;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct ProviderScan {
    pub generation: Uuid,
    pub checkpoint: String,
    pub cursor: Option<String>,
    pub completed: bool,
}

impl Database {
    pub fn provider_scan(&self) -> Result<Option<ProviderScan>, DatabaseError> {
        let row: Option<(String, String, Option<String>, bool)> =
            self.connection.query_row("SELECT generation,checkpoint,cursor,completed FROM provider_scan WHERE singleton=1", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).optional()?;
        row.map(|(generation, checkpoint, cursor, completed)| Ok(ProviderScan { generation: generation.parse().map_err(DatabaseError::operation)?, checkpoint, cursor, completed })).transpose()
    }

    pub fn begin_provider_scan(&self, checkpoint: String, cursor: String) -> Result<ProviderScan, DatabaseError> {
        let generation = Uuid::new_v4();
        self.write(|tx| {
            tx.execute("INSERT INTO provider_scan(singleton,generation,checkpoint,cursor,completed) VALUES(1,?1,?2,?3,0) ON CONFLICT(singleton) DO UPDATE SET generation=excluded.generation,checkpoint=excluded.checkpoint,cursor=excluded.cursor,completed=0", params![generation.to_string(), checkpoint, cursor])?;
            Ok(())
        })?;
        Ok(ProviderScan { generation, checkpoint, cursor: Some(cursor), completed: false })
    }

    /// Commit a page only if this is still the captured scan position. Network
    /// requests and JSON processing happen outside the writer transaction.
    pub fn commit_provider_scan_page(&self, captured: &ProviderScan, next_cursor: Option<String>, entries: Vec<(String, String)>) -> Result<bool, DatabaseError> {
        self.write(|tx| {
            let current: bool =
                tx.query_row("SELECT EXISTS(SELECT 1 FROM provider_scan WHERE singleton=1 AND generation=?1 AND cursor IS ?2 AND NOT completed)", params![captured.generation.to_string(), captured.cursor], |row| row.get(0))?;
            if !current {
                return Ok(false);
            }
            for (file_id, entry) in entries {
                tx.execute(
                    "INSERT INTO provider_scan_entry(file_id,generation,entry) VALUES(?1,?2,?3) ON CONFLICT(file_id) DO UPDATE SET generation=excluded.generation,entry=excluded.entry",
                    params![file_id, captured.generation.to_string(), entry],
                )?;
            }
            tx.execute("UPDATE provider_scan SET cursor=?1 WHERE singleton=1", [next_cursor])?;
            Ok(true)
        })
    }

    pub fn provider_scan_entries(&self, generation: Uuid, after: &str, limit: usize) -> Result<Vec<(String, String)>, DatabaseError> {
        let mut query = self.connection.prepare("SELECT file_id,entry FROM provider_scan_entry WHERE generation=?1 AND file_id>?2 ORDER BY file_id LIMIT ?3")?;
        query.query_map(params![generation.to_string(), after, limit.min(1000) as i64], |row| Ok((row.get(0)?, row.get(1)?)))?.collect::<Result<Vec<_>, _>>().map_err(DatabaseError::from)
    }

    pub fn finish_provider_scan(&self, generation: Uuid, checkpoint: &str) -> Result<bool, DatabaseError> {
        self.write(|tx| {
            let updated = tx.execute("UPDATE provider_scan SET checkpoint=?1,completed=1 WHERE singleton=1 AND generation=?2 AND cursor IS NULL", params![checkpoint, generation.to_string()])?;
            if updated == 0 {
                return Ok(false);
            }
            tx.execute("DELETE FROM provider_scan_entry WHERE generation!=?1", [generation.to_string()])?;
            Ok(true)
        })
    }

    pub fn advance_provider_checkpoint(&self, generation: Uuid, checkpoint: &str) -> Result<bool, DatabaseError> {
        Ok(self.connection.execute("UPDATE provider_scan SET checkpoint=?1 WHERE singleton=1 AND generation=?2 AND completed", params![checkpoint, generation.to_string()])? != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_scan_resumes_and_rejects_stale_page_completion() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let first = db.begin_provider_scan("checkpoint".into(), "page-1".into()).unwrap();
        assert!(!db.finish_provider_scan(first.generation, "premature").unwrap());
        assert!(db.commit_provider_scan_page(&first, Some("page-2".into()), vec![("book-a".into(), "first".into())]).unwrap());
        assert!(!db.commit_provider_scan_page(&first, None, vec![("book-b".into(), "stale".into())]).unwrap());
        let resumed = db.provider_scan().unwrap().unwrap();
        assert_eq!(resumed.cursor.as_deref(), Some("page-2"));
        assert!(!resumed.completed);
        assert!(db.commit_provider_scan_page(&resumed, None, vec![("book-b".into(), "second".into())]).unwrap());
        assert!(db.finish_provider_scan(first.generation, "next-checkpoint").unwrap());
        assert_eq!(db.provider_scan_entries(first.generation, "", 100).unwrap(), vec![("book-a".into(), "first".into()), ("book-b".into(), "second".into())]);
        assert_eq!(db.provider_scan().unwrap().unwrap().checkpoint, "next-checkpoint");
        assert!(!db.commit_provider_scan_page(&resumed, None, Vec::new()).unwrap());
    }

    #[test]
    fn replacement_scan_retains_unseen_previous_entries_until_it_completes() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let old = db.begin_provider_scan("old".into(), "start".into()).unwrap();
        db.commit_provider_scan_page(&old, None, vec![("old-book".into(), "old".into())]).unwrap();
        db.finish_provider_scan(old.generation, "old-finished").unwrap();
        let new = db.begin_provider_scan("new".into(), "start".into()).unwrap();
        db.commit_provider_scan_page(&new, Some("more".into()), vec![("new-book".into(), "new".into())]).unwrap();
        assert_eq!(db.provider_scan_entries(old.generation, "", 100).unwrap().len(), 1);
        assert!(!db.finish_provider_scan(old.generation, "stale").unwrap());
        assert!(!db.advance_provider_checkpoint(new.generation, "premature").unwrap());
        let resumed = db.provider_scan().unwrap().unwrap();
        db.commit_provider_scan_page(&resumed, None, Vec::new()).unwrap();
        db.finish_provider_scan(new.generation, "finished").unwrap();
        assert!(db.provider_scan_entries(old.generation, "", 100).unwrap().is_empty());
        assert_eq!(db.provider_scan_entries(new.generation, "", 100).unwrap(), vec![("new-book".into(), "new".into())]);
    }
}
