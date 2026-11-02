//! Transfer planning, snapshots, and conditional settlements.
use library_model::BookUploadIntent;
use std::collections::HashSet;

use crate::placements::{FolderPaths, stored_book_path_column};
use crate::query_methods::query_rows;
use crate::{Database, DatabaseError};
// ---- planning ----
// Typed asset-planning reads and conditional settlements.
//
// Transfer orchestration receives durable snapshots from here; it never
// checks out SQLite or chooses transaction boundaries itself.

use super::TransfersSql;
use sync_common::BookId;

#[derive(Clone, Debug)]
pub struct BookUploadSnapshot {
    pub local_versions: Vec<(String, String, String)>,
    pub intent: Option<BookUploadIntent>,
}

impl Database {
    /// One bounded page of covers to fetch. Skip books without a local path;
    /// advance the SQL cursor internally so skipped rows cannot hide later work.
    pub fn pending_thumbnail_downloads(&self, after: Option<&BookId>, limit: i64) -> Result<Vec<BookId>, DatabaseError> {
        let mut after = after.copied();
        loop {
            let page = query_rows(|read| self.connection.pending_thumbnail_downloads(after.as_ref(), limit, read), |row| row.get::<_, BookId>(0))?;
            let Some(last) = page.last() else { return Ok(Vec::new()) };
            after = Some(*last);

            // Batch path reads and reuse folder ancestry within one read snapshot.
            let snapshot = self.connection.is_autocommit().then(|| self.connection.unchecked_transaction()).transpose()?;
            let requested = serde_json::to_string(&page).map_err(DatabaseError::operation)?;
            let mut eligible = HashSet::new();
            self.connection.transfer_snapshot_projection_paths(&requested, |row| {
                stored_book_path_column(row, 1)?;
                eligible.insert(row.get::<_, BookId>(0)?);
                Ok(())
            })?;
            let placements = query_rows(|callback| self.connection.transfer_snapshot_versions(&requested, callback), |row| Ok((row.get::<_, BookId>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?;
            let mut folders = FolderPaths::new(&self.connection);
            for (hash, directory, file_name) in placements {
                if folders.placement(&directory, &file_name)?.is_some() {
                    eligible.insert(hash);
                }
            }
            if let Some(snapshot) = snapshot {
                snapshot.commit()?;
            }
            let eligible: Vec<_> = page.into_iter().filter(|hash| eligible.contains(hash)).collect();
            if !eligible.is_empty() {
                return Ok(eligible);
            }
        }
    }
}
