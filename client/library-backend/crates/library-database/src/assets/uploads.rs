pub(crate) use super::TransfersSql;
use crate::query_methods::{query_last, query_rows};
pub(crate) use crate::*;
pub(crate) use library_model::BlobKind;
pub(crate) use library_model::BookUploadIntent;
pub(crate) use sync_common::BookId;

impl Database {
    /// Metadata remains available for cloud books without a local file.
    pub fn book_storage_titles(&self, books: &[BookId]) -> Result<Vec<(BookId, String)>, DatabaseError> {
        let requested = serde_json::to_string(books).map_err(DatabaseError::operation)?;
        let mut query = self.connection.prepare_cached(
            "SELECT b.content_hash, COALESCE(NULLIF(trim(b.title),''),
                (SELECT NULLIF(bd.file_name,'') FROM book_dir bd WHERE bd.book_row_id=b.row_id ORDER BY bd.deleted_at IS NOT NULL,bd.dir_id LIMIT 1))
             FROM json_each(?1) requested JOIN book b ON b.content_hash=unhex(requested.value)",
        )?;
        Ok(query.query_map([requested], |row| Ok((row.get(0)?, row.get::<_, Option<String>>(1)?.unwrap_or_default())))?.collect::<Result<_, _>>()?)
    }

    /// Distinct local books, including uploads not yet prepared by file replay.
    /// Remote presence lets the account avoid charging shared books twice.
    pub fn cloud_quota_books(&self) -> Result<Vec<library_model::CloudQuotaBook>, DatabaseError> {
        let mut query = self.connection.prepare_cached(
            "SELECT b.content_hash, COALESCE(c.size_bytes,MAX(bd.size_bytes),0), COALESCE(t.server_has_book,0), COALESCE(c.canonical_hash,o.canonical_hash,t.upload_checksum,MAX(f.byte_hash),CASE WHEN length(b.content_hash)=32 THEN b.content_hash END)
             FROM book b JOIN book_dir bd ON bd.book_row_id=b.row_id
             LEFT JOIN book_transfer t ON t.content_hash=b.content_hash
             LEFT JOIN book_content c ON c.book_id=b.content_hash
             LEFT JOIN local_content_origin o ON o.book_id=b.content_hash
             LEFT JOIN local_file_content f ON f.book_id=b.content_hash AND f.dir_id=bd.dir_id AND bd.is_downloaded=1
             WHERE b.deleted_at IS NULL AND bd.deleted_at IS NULL AND (bd.is_downloaded=1 OR t.server_has_book)
             GROUP BY b.content_hash",
        )?;
        Ok(query
            .query_map([], |row| Ok(library_model::CloudQuotaBook { book_id: row.get(0)?, size_bytes: crate::sync::nonnegative_u64(row.get(1)?, 1, "book size")?, server_has_book: row.get(2)?, byte_checksum: row.get(3)? }))?
            .collect::<Result<_, _>>()?)
    }

    pub(crate) fn upload_intent_of(connection: &rusqlite::Connection, hash: &BookId) -> Result<Option<BookUploadIntent>, DatabaseError> {
        Ok(query_last(|read| connection.transfer_upload_intent(hash, read), |row| Ok(BookUploadIntent { byte_checksum: row.get(2)?, id: row.get(0)?, book_id: *hash, size_bytes: row.get::<_, i64>(1)? as u64 }))?)
    }

    pub(crate) fn complete_book_upload_in_transaction(connection: &rusqlite::Connection, hash: &BookId, expected_versions: &[(String, String, String)], intent: Option<&BookUploadIntent>) -> Result<bool, DatabaseError> {
        let mut expected = expected_versions.iter();
        let mut current = true;
        let requested = serde_json::to_string(&[hash]).map_err(DatabaseError::operation)?;
        connection.transfer_snapshot_versions(&requested, |row| {
            let version = (row.get(1)?, row.get(2)?, row.get(3)?);
            current &= expected.next() == Some(&version);
            Ok(())
        })?;
        current &= expected.next().is_none();
        if current {
            if let Some(checksum) = intent.and_then(|intent| intent.byte_checksum).or_else(|| hash.content_hash()) {
                crate::content_identity::freeze_content(connection, *hash, checksum, intent.map(|intent| intent.size_bytes))?;
                crate::sync::apply::registers::record_local(connection, library_replica::LibraryStateEntry::BookContent { book_id: *hash, checksum, size_bytes: intent.map(|intent| intent.size_bytes), pdf: None })?;
            }
            connection.record_remote_asset(BlobKind::Book.storage(), hash)?;
            if let Some(intent) = intent {
                connection.transfer_complete_upload(intent.id, &intent.book_id)?;
            }
        } else {
            connection.forget_remote_asset(BlobKind::Book.storage(), hash)?;
        }
        Ok(current)
    }

    /// One page of prepared uploads by intent id.
    pub fn pending_book_uploads(&self, after: i64, limit: i64) -> Result<Vec<BookUploadIntent>, DatabaseError> {
        Ok(query_rows(|read| self.connection.transfer_pending_book_uploads(after, limit, read), |row| Ok(BookUploadIntent { byte_checksum: row.get(3)?, id: row.get(0)?, book_id: row.get::<_, BookId>(1)?, size_bytes: row.get::<_, i64>(2)? as u64 }))?)
    }

    /// One page of eligible covers by content hash.
    pub fn pending_cover_uploads(&self, after: Option<&BookId>, limit: i64) -> Result<Vec<BookId>, DatabaseError> {
        Ok(query_rows(|read| self.connection.transfer_pending_cover_uploads(after, limit, read), |row| row.get(0))?)
    }
}
