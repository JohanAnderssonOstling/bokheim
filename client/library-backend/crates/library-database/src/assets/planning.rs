//! Transfer planning, snapshots, and conditional settlements.
use library_model::BookUploadIntent;
use std::collections::HashSet;

use crate::placements::{FolderPaths, validate_stored_book_path, validate_stored_placement};
use crate::query_methods::{invalid_text_column, query_rows, text_column, try_for_each_row};
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
            let mut page = query_rows(|read| self.connection.pending_thumbnail_downloads(after.as_ref(), limit, read), |row| row.get::<_, BookId>(0))?;
            let Some(last) = page.last() else { return Ok(Vec::new()) };
            after = Some(*last);

            // Batch path reads and reuse folder ancestry within one read snapshot.
            let snapshot = self.connection.is_autocommit().then(|| self.connection.unchecked_transaction()).transpose()?;
            let requested = serde_json::to_string(&page).map_err(DatabaseError::operation)?;
            let mut eligible = HashSet::new();
            self.connection.thumbnail_download_projection_paths(&requested, |row| {
                let path = text_column(row, 1)?;
                validate_stored_book_path(path).map_err(|error| invalid_text_column(1, error.to_string()))?;
                eligible.insert(row.get::<_, BookId>(0)?);
                Ok(())
            })?;
            let mut folders = FolderPaths::for_books(&self.connection, &requested)?;
            try_for_each_row(
                |read| self.connection.thumbnail_download_placements(&requested, read),
                |row| {
                    let hash = row.get::<_, BookId>(0)?;
                    let directory = text_column(row, 1)?;
                    let file_name = text_column(row, 2)?;
                    if let Some(folder) = folders.folder(directory)? {
                        validate_stored_placement(folder, file_name)?;
                        eligible.insert(hash);
                    }
                    Ok(())
                },
            )?;
            if let Some(snapshot) = snapshot {
                snapshot.commit()?;
            }
            page.retain(|hash| eligible.contains(hash));
            if !page.is_empty() {
                return Ok(page);
            }
        }
    }
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn pending_thumbnail_downloads(after: Option<&BookId>, limit: i64) -> Vec<BookId>;
    arguments (Option<BookId>, i64,) = (after.cloned(), limit,);
    execute database, arguments => database.pending_thumbnail_downloads(arguments.0.as_ref(), arguments.1);
}
