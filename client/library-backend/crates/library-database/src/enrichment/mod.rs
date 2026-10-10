//! Thumbnail jobs, upload preparation, and the metadata server's answers.

use std::collections::HashSet;
pub(crate) use crate::shared_sql::SharedSql;
// ---- enrichment ----
// Typed thumbnail-work queue operations.

pub(crate) use crate::{Database, DatabaseError};
pub(crate) use include_sqlite_sql::include_sql;
pub(crate) use sync_common::ContentHash;

pub mod facts;
pub use facts::EnrichmentCandidate;

impl Database {
    pub fn pending_thumbnail_job_count(&self) -> Result<u64, DatabaseError> {
        let connection = &self.connection;
        Ok(connection.thumbnail_pending_job_count(|row| row.get::<_, i64>(0))?.max(0) as u64)
    }

    pub fn thumbnail_book_format(&self, hash: ContentHash) -> Result<Option<book_model::BookFormat>, DatabaseError> {
        let connection = &self.connection;
        let mut format = None;
        connection.thumbnail_local_format(&hash, |row| {
            format = Some(row.get::<_, Option<i64>>(0)?);
            Ok(())
        })?;
        format.map(|format| format.and_then(book_model::BookFormat::from_code).ok_or_else(|| DatabaseError::message("book format is missing"))).transpose()
    }

    /// Records a cover made on this device, or that the book has none.
    pub fn complete_thumbnail_work(&self, hash: &ContentHash, has_cover: bool) -> Result<(), DatabaseError> {
        self.connection.thumbnail_set_result(hash, if has_cover { "ready" } else { "no_cover" })?;
        Ok(())
    }

    /// Whether the server holds this book's cover.
    pub fn server_has_cover(&self, hash: &ContentHash) -> Result<bool, DatabaseError> {
        Ok(self.connection.thumbnail_has_remote(hash, |row| row.get(0))?)
    }

    /// Ends cover work without a cover made here.
    pub fn discard_thumbnail_work(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.connection.thumbnail_discard(hash)?;
        Ok(())
    }

    /// The next page of books with a live placement after `after`, whose
    /// cover presence on the server can be checked.
    pub fn thumbnail_remote_presence_candidates(&self, after: Option<&ContentHash>) -> Result<Vec<ContentHash>, DatabaseError> {
        let mut hashes = Vec::new();
        self.connection.thumbnail_remote_presence_candidates(after, |row| {
            hashes.push(row.get::<_, ContentHash>(0)?);
            Ok(())
        })?;
        Ok(hashes)
    }

    pub fn request_missing_thumbnail(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        let connection = &self.connection;
        connection.thumbnail_request_missing(hash)?;
        Ok(())
    }

    /// Queues thumbnail recovery unconditionally.
    /// Unlike [`Database::request_missing_thumbnail`], which only revives
    /// finished work, this also covers books that never had a work row.
    pub fn shared_request_thumbnail(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.connection.shared_request_thumbnail(hash)?;
        Ok(())
    }

    /// Current thumbnail work state, if any row exists.
    pub fn thumbnail_work_state(&self, hash: &ContentHash) -> Result<Option<String>, DatabaseError> {
        let mut state = None;
        self.connection.thumbnail_work_state(hash, |row| {
            state = Some(row.get::<_, String>(0)?);
            Ok(())
        })?;
        Ok(state)
    }

}

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
            members.push((row.get::<_, ContentHash>(0)?, row.get::<_, Option<i64>>(1)?));
            Ok(())
        })?;
        Ok(UploadPreparationBatch { members })
    }

    pub fn prepared_upload_batch(&self, scope: &HashSet<ContentHash>) -> Result<UploadPreparationBatch, DatabaseError> {
        let scope_json = serde_json::to_string(scope).map_err(DatabaseError::operation)?;
        let affected_dirs = serde_json::to_string(&crate::placements::pending_work_directories(&self.connection)?).map_err(DatabaseError::operation)?;
        let mut members = Vec::new();
        self.connection.prepared_upload_batch(scope_json.as_str(), &affected_dirs, |row| {
            members.push((row.get::<_, ContentHash>(0)?, row.get::<_, Option<i64>>(1)?));
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

    pub fn upload_thumbnail_candidates(&self, members: &HashSet<ContentHash>) -> Result<Vec<ContentHash>, DatabaseError> {
        let scope_json = serde_json::to_string(members).map_err(DatabaseError::operation)?;
        let mut hashes = Vec::new();
        self.connection.upload_thumbnail_candidates(scope_json.as_str(), |row| {
            hashes.push(row.get::<_, ContentHash>(0)?);
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
    fn obsolete_retry_policy_is_reset_once_and_current_work_survives_reopening() {
        let (db, path) = open_test_database();
        let hash = ContentHash::new(&"c".repeat(64));
        db.connection.fixtures_insert_book_5(&(&hash)).unwrap();
        db.connection.fixtures_seed_draft_retry(&(&hash)).unwrap();
        db.connection.fixtures_seed_cover_check(&(&hash)).unwrap();
        db.connection.fixtures_update_library_local_state_enrichment_policy().unwrap();
        drop(db);
        let db = Database::open(&path).unwrap();
        db.initialize_library().unwrap();
        assert_eq!(db.connection.fixtures_read_enrichment_work(|r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, bool>(1)?))).unwrap(), (None, false));
        db.connection.fixtures_seed_draft_retry(&(&hash)).unwrap();
        db.connection.fixtures_seed_cover_check(&(&hash)).unwrap();
        drop(db);
        let db = Database::open(&path).unwrap();
        db.initialize_library().unwrap();
        assert_eq!(db.connection.fixtures_read_enrichment_work(|r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, bool>(1)?))).unwrap(), (Some(999999), true));
    }

}

#[cfg(test)]
use crate::sql_fixtures::FixturesSql;
