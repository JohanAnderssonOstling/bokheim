pub(crate) use super::TransfersSql;
pub(crate) use crate::shared_sql::SharedSql;
pub(crate) use crate::*;
pub(crate) use library_model::BlobKind;
pub(crate) use library_model::BookUploadIntent;
pub(crate) use sync_common::ContentHash;

impl Database {
    pub(crate) fn upload_intent_of(connection: &rusqlite::Connection, hash: &ContentHash) -> Result<Option<BookUploadIntent>, DatabaseError> {
        let mut intent = None;
        connection.transfer_upload_intent(hash.as_str(), |row| {
            intent = Some(BookUploadIntent { id: row.get(0)?, content_hash: hash.clone(), checksum: ContentHash::new(&row.get::<_, String>(1)?), size_bytes: row.get::<_, i64>(2)? as u64 });
            Ok(())
        })?;
        Ok(intent)
    }

    pub(crate) fn complete_book_upload_in_transaction(connection: &rusqlite::Connection, hash: &ContentHash, expected_versions: &[(String, String, String)], intent: Option<&BookUploadIntent>) -> Result<bool, DatabaseError> {
        let current = {
            let mut versions: Vec<(String, String, String)> = Vec::new();
            let requested = serde_json::to_string(&[hash.as_str()]).map_err(DatabaseError::operation)?;
            connection.transfer_snapshot_versions(&requested, |row| {
                versions.push((row.get(1)?, row.get(2)?, row.get(3)?));
                Ok(())
            })?;
            versions
        } == expected_versions;
        if current {
            connection.record_remote_asset(BlobKind::Book.storage(), hash.as_str())?;
            if let Some(intent) = intent {
                connection.transfer_complete_upload(intent.id, intent.content_hash.as_str(), intent.checksum.as_str())?;
            }
        } else {
            connection.forget_remote_asset(BlobKind::Book.storage(), hash.as_str())?;
        }
        Ok(current)
    }

    pub fn book_upload_rejection_reason(&self, hash: &ContentHash) -> Result<Option<String>, DatabaseError> {
        let mut reason = None;
        self.connection.transfer_book_upload_rejection_reason(hash.as_str(), super::rejection_cutoff()?, |row| {
            reason = row.get(0)?;
            Ok(())
        })?;
        Ok(reason)
    }

    pub fn book_upload_intent(&self, hash: &ContentHash) -> Result<Option<BookUploadIntent>, DatabaseError> {
        {
            Database::upload_intent_of(&self.connection, hash)
        }
    }
}
