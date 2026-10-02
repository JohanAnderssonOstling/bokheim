//! Thumbnail jobs, upload preparation, and the metadata server's answers.

use std::collections::HashSet;
pub(crate) use crate::shared_sql::SharedSql;
pub(crate) use crate::assets::TransfersSql;
// ---- enrichment ----
// Typed thumbnail-work queue operations.

pub(crate) use crate::{Database, DatabaseError};
pub(crate) use include_sqlite_sql::include_sql;
pub(crate) use sync_common::ContentHash;

pub mod facts;
pub(crate) use facts::current_answer;
pub use facts::EnrichmentCandidate;

impl Database {
    pub fn pending_thumbnail_job_count(&self) -> Result<u64, DatabaseError> {
        let connection = &self.connection;
        Ok(connection.thumbnail_pending_job_count(|row| row.get::<_, i64>(0))?.max(0) as u64)
    }

    

    pub fn thumbnail_book_format(&self, hash: ContentHash) -> Result<Option<book_model::BookFormat>, DatabaseError> {
        let connection = &self.connection;
        let mut format = None;
        connection.thumbnail_local_format(hash.as_str(), |row| {
            format = Some(row.get::<_, Option<i64>>(0)?);
            Ok(())
        })?;
        format.map(|format| format.and_then(book_model::BookFormat::from_code).ok_or_else(|| DatabaseError::message("book format is missing"))).transpose()
    }

    

    

    pub fn thumbnail_download_coverage(&self) -> Result<(u64, u64), DatabaseError> {
        let mut coverage = (0, 0);
        self.connection.thumbnail_download_coverage(|row| {
            coverage = (row.get::<_, i64>(0)?.max(0) as u64, row.get::<_, i64>(1)?.max(0) as u64);
            Ok(())
        })?;
        Ok(coverage)
    }

    

    

    

    /// Records a cover made on this device, or that the book has none.
    pub fn complete_thumbnail_work(&self, hash: &ContentHash, has_cover: bool) -> Result<(), DatabaseError> {
        self.connection.thumbnail_set_result(hash.as_str(), if has_cover { "ready" } else { "no_cover" })?;
        Ok(())
    }

    

    

    pub fn thumbnail_upload_allowed(&self, hash: &ContentHash) -> Result<bool, DatabaseError> {
        let connection = &self.connection;
        let mut allowed = false;
        connection.thumbnail_upload_allowed(hash.as_str(), crate::assets::rejection_cutoff()?, |row| {
            allowed = row.get(0)?;
            Ok(())
        })?;
        Ok(allowed)
    }

    pub fn complete_downloaded_thumbnail(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.write(|transaction| Self::complete_downloaded_thumbnail_on(transaction, hash))
    }

    pub(crate) fn complete_downloaded_thumbnail_on(connection: &rusqlite::Connection, hash: &ContentHash) -> Result<(), DatabaseError> {
        connection.record_remote_asset("thumbnail", hash.as_str())?;
        connection.thumbnail_set_result(hash.as_str(), "ready")?;
        Ok(())
    }

    pub fn request_missing_thumbnail(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        let connection = &self.connection;
        connection.thumbnail_request_missing(hash.as_str())?;
        Ok(())
    }

    /// Queues thumbnail recovery unconditionally, resetting any backoff.
    /// Unlike [`Database::request_missing_thumbnail`], which only revives
    /// finished work, this also covers books that never had a work row.
    pub fn shared_request_thumbnail(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.connection.shared_request_thumbnail(hash.as_str())?;
        Ok(())
    }

    /// Current thumbnail work state and retry deadline, if any row exists.
    pub fn thumbnail_work_state(&self, hash: &ContentHash) -> Result<Option<(String, i64)>, DatabaseError> {
        let mut state = None;
        self.connection.thumbnail_work_state(hash.as_str(), |row| {
            state = Some((row.get::<_, String>(0)?, row.get::<_, i64>(1)?));
            Ok(())
        })?;
        Ok(state)
    }

    
}

include_sql!("src/enrichment/sql/schema/tables.sql");
include_sql!("src/enrichment/sql/schema/index.sql");
include_sql!("src/enrichment/sql/enrichment.sql");

// ---- work ----
// Upload preparation snapshots.

#[derive(Clone, Debug)]
pub struct UploadPreparationBatch {
    pub members: Vec<(ContentHash, Option<i64>)>,
}

impl UploadPreparationBatch {
    pub fn hashes(&self) -> HashSet<ContentHash> {
        self.members.iter().map(|(hash, _)| *hash).collect()
    }
}

impl Database {
    pub fn upload_preparation_snapshot(&self) -> Result<UploadPreparationBatch, DatabaseError> {
        let mut members = Vec::new();
        self.connection.upload_preparation_snapshot(|row| {
            members.push((ContentHash::new(&row.get::<_, String>(0)?), row.get::<_, Option<i64>>(1)?));
            Ok(())
        })?;
        Ok(UploadPreparationBatch { members })
    }

    pub fn prepared_upload_batch(&self, scope: &HashSet<ContentHash>) -> Result<UploadPreparationBatch, DatabaseError> {
        let scope_json = serde_json::to_string(scope).map_err(DatabaseError::operation)?;
        let affected_dirs = serde_json::to_string(&crate::placements::pending_work_directories(&self.connection)?).map_err(DatabaseError::operation)?;
        let mut members = Vec::new();
        self.connection.prepared_upload_batch(scope_json.as_str(), &affected_dirs, |row| {
            members.push((ContentHash::new(&row.get::<_, String>(0)?), row.get::<_, Option<i64>>(1)?));
            Ok(())
        })?;
        Ok(UploadPreparationBatch { members })
    }

    pub fn finish_upload_preparation(&self, batch: &UploadPreparationBatch) -> Result<(), DatabaseError> {
        let members: Vec<(String, Option<i64>)> = batch.members.iter().map(|(hash, id)| (hash.to_string(), *id)).collect();
        let json = serde_json::to_string(&members).map_err(DatabaseError::operation)?;
        self.connection.finish_upload_preparation(json.as_str())?;
        Ok(())
    }

    pub fn upload_thumbnail_candidates(&self, members: &HashSet<ContentHash>, now: i64) -> Result<Vec<ContentHash>, DatabaseError> {
        let scope_json = serde_json::to_string(members).map_err(DatabaseError::operation)?;
        let mut hashes = Vec::new();
        self.connection.upload_thumbnail_candidates(scope_json.as_str(), now, |row| {
            hashes.push(ContentHash::new(&row.get::<_, String>(0)?));
            Ok(())
        })?;
        Ok(hashes)
    }
}

#[cfg(test)]
mod schema_tests {
    use super::*;

    fn open_test_database() -> (Database, std::path::PathBuf) {
        // Process IDs can repeat across test runs while old fixture files remain.
        let path = std::env::temp_dir().join(format!("library-database-enrichment-schema-{}", uuid::Uuid::new_v4()));
        let database = Database::open(&path).expect("open test database");
        database.initialize_library().expect("initialize test library");
        (database, path)
    }

    #[test]
    fn old_answers_are_discarded_once_and_current_answers_survive_reopening() {
        let (db, path) = open_test_database();
        let hash = ContentHash::new(&"c".repeat(64));
        db.connection.execute("INSERT INTO book(content_hash,title,format) VALUES(?1,'Book',1)", [hash.as_str()]).unwrap();
        db.connection.execute("INSERT INTO book_enrichment(content_hash,request_key,status,response,retry_at) VALUES(?1,'key','found','old ISBN cover answer',999999)", [hash.as_str()]).unwrap();
        db.connection.execute("UPDATE library_local_state SET enrichment_policy=NULL WHERE singleton=1", []).unwrap();
        drop(db);
        let db = Database::open(&path).unwrap();
        db.initialize_library().unwrap();
        assert_eq!(db.connection.query_row("SELECT count(*) FROM book_enrichment", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        db.connection.execute("INSERT INTO book_enrichment(content_hash,request_key,status,retry_at) VALUES(?1,'key','no_match',999999)", [hash.as_str()]).unwrap();
        drop(db);
        let db = Database::open(&path).unwrap();
        db.initialize_library().unwrap();
        assert_eq!(db.connection.query_row("SELECT count(*) FROM book_enrichment", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    }

    #[test]
    fn remote_thumbnail_presence_does_not_strand_pending_recovery() {
        let (db, _path) = open_test_database();
        let hash = ContentHash::new(&"a".repeat(64));
        db.connection.execute("INSERT INTO book(content_hash,title,format) VALUES(?1,'A book',1)", [hash.as_str()]).unwrap();
        db.connection.execute("INSERT INTO book_transfer(content_hash,cover_state,server_has_cover) VALUES(?1,'pending',1)", [hash.as_str()]).unwrap();

        assert_eq!(db.upload_thumbnail_candidates(&HashSet::from([hash]), 0).unwrap(), [hash]);
    }

    #[test]
    fn thumbnail_download_coverage_survives_reopening_and_tracks_recovery() {
        let (db, path) = open_test_database();
        let first = ContentHash::new(&"a".repeat(64));
        let second = ContentHash::new(&"b".repeat(64));
        for hash in [first, second] {
            db.connection.execute("INSERT INTO book(content_hash,title,format) VALUES(?1,'A book',1)", [hash.as_str()]).unwrap();
            db.connection.execute("INSERT INTO book_transfer(content_hash,server_has_cover) VALUES(?1,1)", [hash.as_str()]).unwrap();
        }
        assert_eq!(db.thumbnail_download_coverage().unwrap(), (0, 2));
        db.complete_downloaded_thumbnail(&first).unwrap();
        assert_eq!(db.thumbnail_download_coverage().unwrap(), (1, 2));
        drop(db);

        let db = Database::open(&path).unwrap();
        assert_eq!(db.thumbnail_download_coverage().unwrap(), (1, 2));
        db.complete_downloaded_thumbnail(&second).unwrap();
        assert_eq!(db.thumbnail_download_coverage().unwrap(), (2, 2));
        db.request_missing_thumbnail(&first).unwrap();
        assert_eq!(db.thumbnail_download_coverage().unwrap(), (1, 2));
    }
}
