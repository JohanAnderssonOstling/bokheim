//! Transfer planning, snapshots, and conditional settlements.
use library_model::BookUploadIntent;

use crate::*;
// ---- planning ----
// Typed asset-planning reads and conditional settlements.
//
// Transfer orchestration receives durable snapshots from here; it never
// checks out SQLite or chooses transaction boundaries itself.

use super::TransfersSql;
use sync_common::ContentHash;

#[derive(Clone, Debug)]
pub struct BookUploadSnapshot {
    pub local_versions: Vec<(String, String, String)>,
    pub intent: Option<BookUploadIntent>,
}

impl Database {
    /// One bounded page of covers to fetch. Skip books without a local path;
    /// advance the SQL cursor internally so skipped rows cannot hide later work.
    pub fn pending_thumbnail_downloads(&self, after: Option<&ContentHash>, limit: i64) -> Result<Vec<ContentHash>, DatabaseError> {
        let mut after = after.copied();
        loop {
            let mut page = Vec::new();
            self.connection.pending_thumbnail_downloads(after.as_ref(), limit, |row| {
                page.push(row.get::<_, ContentHash>(0)?);
                Ok(())
            })?;
            let Some(last) = page.last() else { return Ok(Vec::new()) };
            after = Some(*last);
            let mut eligible = Vec::new();
            for hash in page {
                if !self.book_paths(&hash)?.is_empty() {
                    eligible.push(hash);
                }
            }
            if !eligible.is_empty() {
                return Ok(eligible);
            }
        }
    }
}
