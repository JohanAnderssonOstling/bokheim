pub(crate) use super::TransfersSql;
use crate::query_methods::{query_last, query_rows};
pub(crate) use crate::*;
use content_address::ContentHash;
use library_model::{BookUploadIntent, CloudQuotaBook, RelativeBookPath};
pub(crate) use sync_common::BookId;

/// Metadata for inspecting local bytes after releasing the database connection.
pub struct BookStorageMetadata {
    pub book: CloudQuotaBook,
    pub paths: Vec<RelativeBookPath>,
    pub checksum: Option<ContentHash>,
}

impl Database {
    /// Gather storage-summary metadata in one read snapshot, before file I/O.
    pub fn book_storage_metadata(&self, for_download: bool) -> Result<Vec<BookStorageMetadata>, DatabaseError> {
        self.with_browse_snapshot(|| {
            self.cloud_quota_books()?
                .into_iter()
                .map(|book| {
                    let paths = self.book_paths(&book.book_id)?;
                    let checksum = if for_download && book.server_has_book { self.book_transfer_state_in_snapshot(book.book_id)?.expected_checksum() } else { None };
                    Ok(BookStorageMetadata { book, paths, checksum })
                })
                .collect()
        })
    }

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
    pub fn cloud_quota_books(&self) -> Result<Vec<CloudQuotaBook>, DatabaseError> {
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
            .query_map([], |row| Ok(CloudQuotaBook { book_id: row.get(0)?, size_bytes: crate::sync::nonnegative_u64(row.get(1)?, 1, "book size")?, server_has_book: row.get(2)?, byte_checksum: row.get(3)? }))?
            .collect::<Result<_, _>>()?)
    }

    pub(crate) fn upload_intent_of(connection: &rusqlite::Connection, hash: &BookId) -> Result<Option<BookUploadIntent>, DatabaseError> {
        Ok(query_last(|read| connection.transfer_upload_intent(hash, read), |row| Ok(BookUploadIntent { byte_checksum: row.get(2)?, id: row.get(0)?, book_id: *hash, size_bytes: row.get::<_, i64>(1)? as u64 }))?)
    }

    /// One page of prepared uploads by intent id.
    pub fn pending_book_uploads(&self, after: i64, limit: i64) -> Result<Vec<BookUploadIntent>, DatabaseError> {
        Ok(query_rows(
            |read| self.connection.transfer_pending_book_uploads(after, limit, read),
            |row| Ok(BookUploadIntent { byte_checksum: row.get(3)?, id: row.get(0)?, book_id: row.get::<_, BookId>(1)?, size_bytes: row.get::<_, i64>(2)? as u64 }),
        )?)
    }

    /// One page of eligible covers by content hash.
    pub fn pending_cover_uploads(&self, after: Option<&BookId>, limit: i64) -> Result<Vec<BookId>, DatabaseError> {
        Ok(query_rows(|read| self.connection.transfer_pending_cover_uploads(after, limit, read), |row| row.get(0))?)
    }
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn book_storage_metadata(for_download: bool) -> Vec<BookStorageMetadata>;
    arguments (bool,) = (for_download,);
    execute database, arguments => database.book_storage_metadata(arguments.0);

    fn book_storage_titles(books: &[BookId]) -> Vec<(BookId, String)>;
    arguments (Vec<BookId>,) = (books.to_vec(),);
    execute database, arguments => database.book_storage_titles(&arguments.0);

    fn cloud_quota_books() -> Vec<CloudQuotaBook>;
    arguments () = ();
    execute database, arguments => database.cloud_quota_books();

    fn pending_book_uploads(after: i64, limit: i64) -> Vec<BookUploadIntent>;
    arguments (i64, i64,) = (after, limit,);
    execute database, arguments => database.pending_book_uploads(arguments.0, arguments.1);

    fn pending_cover_uploads(after: Option<&BookId>, limit: i64) -> Vec<BookId>;
    arguments (Option<BookId>, i64,) = (after.cloned(), limit,);
    execute database, arguments => database.pending_cover_uploads(arguments.0.as_ref(), arguments.1);
}
