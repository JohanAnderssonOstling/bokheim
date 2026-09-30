//! Transfer workflow: planning, snapshot drivers, operation status.

use crate::shared_sql::SharedSql;
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

#[derive(Clone, Debug)]
pub struct BookUploadCompletion {
    pub content_hash: ContentHash,
    pub local_versions: Vec<(String, String, String)>,
    pub intent: BookUploadIntent,
}

/// A rejected asset transfer whose persistence must be settled atomically.
pub enum RejectedAsset<'a> {
    BookDownload,
    BookUpload { intent: Option<&'a BookUploadIntent> },
    ThumbnailUpload,
}

impl Database {
    /// Records why a transfer job was rejected, or clears the outstanding
    /// download request. A stale upload intent (superseded since the job was
    /// planned) is left alone rather than recording a rejection for it.
    pub fn settle_rejected_asset(&self, hash: &ContentHash, asset: RejectedAsset<'_>, reason: &str) -> Result<(), DatabaseError> {
        Self::settle_rejected_asset_on(&self.connection, hash, asset, reason)
    }

    pub(super) fn settle_rejected_asset_on(connection: &rusqlite::Connection, hash: &ContentHash, asset: RejectedAsset<'_>, reason: &str) -> Result<(), DatabaseError> {
        match asset {
            RejectedAsset::BookDownload => {
                connection.shared_transfer_complete_download(hash.as_str())?;
            }
            RejectedAsset::BookUpload { intent } => {
                if intent.is_none() || Self::upload_intent_of(connection, hash)?.as_ref() == intent {
                    connection.transfer_record_book_rejection(hash.as_str(), reason)?;
                }
            }
            RejectedAsset::ThumbnailUpload => {
                connection.transfer_record_thumbnail_rejection(hash.as_str())?;
            }
        }
        Ok(())
    }

    /// Books whose cover this device should download, limited to `scope`
    /// when given. A book with no path on this device needs no cover here.
    pub fn pending_thumbnail_downloads(&self, scope: Option<&[ContentHash]>) -> Result<Vec<ContentHash>, DatabaseError> {
        let scope = scope.map(serde_json::to_string).transpose().map_err(DatabaseError::operation)?;
        let mut after = String::new();
        let mut hashes = Vec::new();
        loop {
            let mut page = Vec::new();
            self.connection.pending_thumbnail_downloads(&after, scope.as_deref(), 512, |row| {
                page.push(ContentHash::new(&row.get::<_, String>(0)?));
                Ok(())
            })?;
            let Some(last) = page.last() else { break };
            after = last.as_str().to_owned();
            for hash in page {
                if !self.book_paths(&hash)?.is_empty() {
                    hashes.push(hash);
                }
            }
        }
        Ok(hashes)
    }
}

#[cfg(test)]
mod planning_tests {
    use crate::*;

    use crate::test_support::TestDatabase;

    fn open_test_database() -> TestDatabase {
        TestDatabase::new("asset-planning")
    }

    #[test]
    fn rejection_reason_needs_a_queued_upload() {
        let holder = open_test_database();
        let database = &holder.database;
        let hash = ContentHash::new(&"0".repeat(64));
        assert_eq!(database.book_upload_rejection_reason(&hash).expect("read reason"), None);
        database.settle_rejected_asset(&hash, RejectedAsset::BookUpload { intent: None }, "quota").expect("settle rejection");
        // The rejection is recorded, but without a live upload row there is
        // nothing to report it against — matching the ported query.
        assert_eq!(database.book_upload_rejection_reason(&hash).expect("reread reason"), None);
    }
}
