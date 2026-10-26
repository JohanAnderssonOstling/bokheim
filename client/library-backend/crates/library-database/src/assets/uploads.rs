pub(crate) use super::TransfersSql;
pub(crate) use crate::*;
pub(crate) use library_model::BlobKind;
pub(crate) use library_model::BookUploadIntent;
pub(crate) use sync_common::BookId;

impl Database {
    pub(crate) fn upload_intent_of(connection: &rusqlite::Connection, hash: &BookId) -> Result<Option<BookUploadIntent>, DatabaseError> {
        let mut intent = None;
        connection.transfer_upload_intent(hash, |row| {
            intent = Some(BookUploadIntent { id: row.get(0)?, content_hash: *hash, size_bytes: row.get::<_, i64>(1)? as u64 });
            Ok(())
        })?;
        Ok(intent)
    }

    pub(crate) fn complete_book_upload_in_transaction(connection: &rusqlite::Connection, hash: &BookId, expected_versions: &[(String, String, String)], intent: Option<&BookUploadIntent>) -> Result<bool, DatabaseError> {
        let current = {
            let mut versions: Vec<(String, String, String)> = Vec::new();
            let requested = serde_json::to_string(&[&hash]).map_err(DatabaseError::operation)?;
            connection.transfer_snapshot_versions(&requested, |row| {
                versions.push((row.get(1)?, row.get(2)?, row.get(3)?));
                Ok(())
            })?;
            versions
        } == expected_versions;
        if current {
            connection.record_remote_asset(BlobKind::Book.storage(), hash)?;
            if let Some(intent) = intent {
                connection.transfer_complete_upload(intent.id, &intent.content_hash)?;
            }
        } else {
            connection.forget_remote_asset(BlobKind::Book.storage(), hash)?;
        }
        Ok(current)
    }

    /// One page of prepared uploads by intent id.
    pub fn pending_book_uploads(&self, after: i64, limit: i64) -> Result<Vec<BookUploadIntent>, DatabaseError> {
        let mut intents = Vec::new();
        self.connection.transfer_pending_book_uploads(after, limit, |row| {
            intents.push(BookUploadIntent { id: row.get(0)?, content_hash: row.get::<_, BookId>(1)?, size_bytes: row.get::<_, i64>(2)? as u64 });
            Ok(())
        })?;
        Ok(intents)
    }

    /// One page of eligible covers by content hash.
    pub fn pending_cover_uploads(&self, after: Option<&BookId>, limit: i64) -> Result<Vec<BookId>, DatabaseError> {
        let mut hashes = Vec::new();
        self.connection.transfer_pending_cover_uploads(after, limit, |row| {
            hashes.push(row.get::<_, BookId>(0)?);
            Ok(())
        })?;
        Ok(hashes)
    }
}
