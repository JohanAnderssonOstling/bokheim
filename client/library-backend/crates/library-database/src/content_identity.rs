//! Stable book references and independently verified byte revisions.

use crate::query_methods::try_for_each_row;
use crate::{Database, DatabaseError};
use book_model::BookFormat;
use content_address::{BookId, ContentHash};
use include_sqlite_sql::include_sql;
use rusqlite::{Connection, OptionalExtension, params};
use sync_common::{DirId, LibraryId, ReplicaId};

include_sql!("src/content_identity_operations.sql");

pub(crate) fn initialize_scope(connection: &Connection, library: Option<LibraryId>, replica: ReplicaId) -> Result<(), DatabaseError> {
    let existing: Option<Vec<u8>> = connection.query_row("SELECT scope FROM local_identity_scope WHERE singleton=1", [], |row| row.get(0)).optional()?;
    let wanted = library.unwrap_or(replica);
    if existing.as_deref() == Some(wanted.as_bytes().as_slice()) || (library.is_none() && existing.is_some()) {
        return Ok(());
    }
    connection.execute("INSERT INTO local_identity_scope(singleton,scope) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET scope=excluded.scope", [wanted.as_bytes().as_slice()])?;
    Ok(())
}

fn identity_scope(connection: &Connection) -> Result<[u8; 16], DatabaseError> {
    let scope: Vec<u8> = connection.query_row("SELECT scope FROM local_identity_scope WHERE singleton=1", [], |row| row.get(0))?;
    scope.try_into().map_err(|_| DatabaseError::message("invalid local identity scope"))
}

pub(crate) fn record_alias(connection: &Connection, book: BookId, checksum: ContentHash) -> Result<(), DatabaseError> {
    connection.execute("INSERT OR IGNORE INTO local_content_alias(byte_hash,book_id) VALUES(?1,?2)", params![checksum, book])?;
    Ok(())
}

pub(crate) fn canonical_hash(connection: &Connection, book: BookId) -> Result<Option<ContentHash>, DatabaseError> {
    Ok(connection.query_row("SELECT canonical_hash FROM book_content WHERE book_id=?1", [book], |row| row.get(0)).optional()?)
}

/// An embedded marker is a hint for locating the original revision. It may
/// constrain an upload, but only a verified server revision becomes canonical.
pub(crate) fn expected_hash(connection: &Connection, book: BookId) -> Result<Option<ContentHash>, DatabaseError> {
    if let Some(checksum) = canonical_hash(connection, book)? {
        return Ok(Some(checksum));
    }
    Ok(connection.query_row("SELECT canonical_hash FROM local_content_origin WHERE book_id=?1", [book], |row| row.get(0)).optional()?)
}

pub(crate) fn record_origin(connection: &Connection, book: BookId, source: Option<book_inspection::PdfSourceIdentity>) -> Result<(), DatabaseError> {
    if let Some(source) = source.filter(|source| source.book_id == book) {
        if let Some(checksum) = source.canonical_hash {
            if canonical_hash(connection, book)?.is_none() {
                connection.execute("INSERT OR IGNORE INTO local_content_origin(book_id,canonical_hash) VALUES(?1,?2)", params![book, checksum])?;
            }
        }
    }
    Ok(())
}

pub(crate) fn freeze_content(connection: &Connection, book: BookId, checksum: ContentHash, size_bytes: Option<u64>) -> Result<(), DatabaseError> {
    if canonical_hash(connection, book)?.is_some_and(|current| current != checksum) {
        return Err(DatabaseError::message("the uploaded content hash of a book cannot change"));
    }
    let size_bytes = size_bytes.map(i64::try_from).transpose().map_err(DatabaseError::operation)?;
    let stored_size: Option<i64> = connection.query_row("SELECT size_bytes FROM book_content WHERE book_id=?1", [book], |row| row.get(0)).optional()?.flatten();
    if size_bytes.is_some_and(|size| size <= 0) || stored_size.zip(size_bytes).is_some_and(|(stored, size)| stored != size) {
        return Err(DatabaseError::message("the uploaded byte length of a book cannot change"));
    }
    connection
        .execute("INSERT INTO book_content(book_id,canonical_hash,size_bytes) VALUES(?1,?2,?3) ON CONFLICT(book_id) DO UPDATE SET size_bytes=COALESCE(book_content.size_bytes,excluded.size_bytes)", params![book, checksum, size_bytes])?;
    connection.execute("UPDATE book_transfer SET upload_checksum=?2,upload_size=COALESCE(?3,upload_size) WHERE content_hash=?1 AND upload_id IS NOT NULL", params![book, checksum, size_bytes])?;
    connection.execute("DELETE FROM local_content_origin WHERE book_id=?1", [book])?;
    record_alias(connection, book, checksum)
}

/// Canonicalize distinct books under the caller's writer transaction.
/// The manifest query has already grouped duplicate upload acknowledgements.
pub(crate) fn freeze_contents(connection: &Connection, contents: &[(BookId, ContentHash, Option<u64>)]) -> Result<(), DatabaseError> {
    if contents.is_empty() {
        return Ok(());
    }
    let contents = contents
        .iter()
        .map(|(book, checksum, size)| {
            let size = size.map(i64::try_from).transpose().map_err(DatabaseError::operation)?;
            if size.is_some_and(|size| size <= 0) {
                return Err(DatabaseError::message("the uploaded byte length of a book cannot change"));
            }
            Ok((*book, *checksum, size))
        })
        .collect::<Result<Vec<_>, DatabaseError>>()?;
    let contents = serde_json::to_string(&contents).map_err(DatabaseError::operation)?;
    try_for_each_row(
        |read| connection.content_validate_batch(&contents, read),
        |row| {
            let current = row.get::<_, Option<ContentHash>>(0)?;
            let size = row.get::<_, Option<i64>>(1)?;
            let checksum = row.get::<_, ContentHash>(2)?;
            let requested_size = row.get::<_, Option<i64>>(3)?;
            if current.is_some_and(|current| current != checksum) {
                return Err(DatabaseError::message("the uploaded content hash of a book cannot change"));
            }
            if size.zip(requested_size).is_some_and(|(stored, requested)| stored != requested) {
                return Err(DatabaseError::message("the uploaded byte length of a book cannot change"));
            }
            Ok(())
        },
    )?;
    connection.content_freeze_batch(&contents)?;
    connection.content_update_uploads_batch(&contents)?;
    connection.content_forget_origins_batch(&contents)?;
    connection.content_record_aliases_batch(&contents)?;
    Ok(())
}

pub(crate) fn observe_content(connection: &Connection, book: BookId, directory: DirId, checksum: ContentHash) -> Result<(), DatabaseError> {
    record_alias(connection, book, checksum)?;
    connection.execute("INSERT INTO local_file_content(book_id,dir_id,byte_hash) VALUES(?1,?2,?3) ON CONFLICT(book_id,dir_id) DO UPDATE SET byte_hash=excluded.byte_hash", params![book, directory.to_string(), checksum])?;
    Ok(())
}

impl Database {
    /// Reuse only the exact device-observed file revision from a prior commit.
    pub fn cached_import_source(&self, path: &str, fingerprint: u64) -> Result<Option<ContentHash>, DatabaseError> {
        Ok(self.connection.query_row("SELECT byte_hash FROM local_import_source WHERE path=?1 AND fingerprint=?2", params![path, fingerprint.to_string()], |row| row.get(0)).optional()?)
    }

    /// A stored checksum is valid for a destination only at its observed revision.
    pub fn local_content_revision(&self, book: BookId, directory: DirId, checksum: ContentHash) -> Result<Option<String>, DatabaseError> {
        Ok(self.connection.query_row("SELECT d.local_hash FROM local_file_content f JOIN book b ON b.content_hash=f.book_id JOIN book_dir d ON d.book_row_id=b.row_id AND d.dir_id=f.dir_id WHERE f.book_id=?1 AND f.dir_id=?2 AND f.byte_hash=?3 AND d.deleted_at IS NULL AND d.is_downloaded=1 AND d.local_hash<>''", params![book, directory.to_string(), checksum], |row| row.get(0)).optional()?)
    }
    /// Resolve a byte-hash reference once at the reader boundary. All reader
    /// state remains keyed by the returned stable identity.
    pub fn resolve_book_identity(&self, reference: BookId) -> Result<BookId, DatabaseError> {
        let exists: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=?1)", [reference], |row| row.get(0))?;
        if exists {
            return Ok(reference);
        }
        match reference.content_hash() {
            Some(checksum) => Ok(self.book_for_local_content(checksum)?.unwrap_or(reference)),
            None => Ok(reference),
        }
    }

    pub fn resolve_scanned_content(&self, checksum: ContentHash, source: Option<book_inspection::PdfSourceIdentity>) -> Result<BookId, DatabaseError> {
        if let Some(book) = self.book_for_local_content(checksum)? {
            return Ok(book);
        }
        if let Some(source) = source {
            let known: Option<(Option<i64>, Option<i64>)> = self.connection.query_row("SELECT format,deleted_at FROM book WHERE content_hash=?1", [source.book_id], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
            if let Some((format, None)) = known {
                let canonical = canonical_hash(&self.connection, source.book_id)?;
                if format == Some(book_model::BookFormat::Pdf as i64) && canonical.zip(source.canonical_hash).is_none_or(|(known, hint)| known == hint) {
                    return Ok(source.book_id);
                }
            } else if known.is_none() {
                // A recovered marker supplies only the stable key. Its claimed
                // canonical hash is confirmed separately by sync or download.
                return Ok(source.book_id);
            }
        }
        let legacy = BookId::from(checksum);
        let known: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=?1 AND deleted_at IS NULL)", [legacy], |row| row.get(0))?;
        if known {
            return Ok(legacy);
        }
        Ok(BookId::for_local_content(&identity_scope(&self.connection)?, checksum))
    }

    pub fn confirm_uploaded_content_with_pdf(&self, book: BookId, checksum: ContentHash, size_bytes: Option<u64>, pdf: Option<pdf_view_common::PdfReaderMetadata>) -> Result<(), DatabaseError> {
        self.write(|tx| {
            use crate::sync::apply::registers::{canonical_body, record_local};
            use library_replica::LibraryStateEntry;
            use sync_common::{StateKey, wire};
            let mut size_bytes = size_bytes;
            let mut pdf = pdf;
            if let Some((bytes, _)) = canonical_body(tx, "book_content", &StateKey::Book(book), "")? {
                if let LibraryStateEntry::BookContent { size_bytes: known_size, pdf: known_pdf, .. } = wire::decode::<LibraryStateEntry>(&bytes, wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? {
                    size_bytes = known_size.or(size_bytes);
                    pdf = known_pdf.or(pdf);
                }
            }
            freeze_content(tx, book, checksum, size_bytes)?;
            record_local(tx, LibraryStateEntry::BookContent { book_id: book, checksum, size_bytes, pdf })?;
            tx.execute("UPDATE book_transfer SET server_has_book=1,upload_id=NULL,upload_size=NULL,upload_prepared=NULL,upload_checksum=NULL WHERE content_hash=?1", [book])?;
            Ok(())
        })
    }

    pub fn confirm_downloaded_content(&self, book: BookId, checksum: ContentHash) -> Result<(), DatabaseError> {
        self.write(|tx| freeze_content(tx, book, checksum, None))
    }

    pub fn canonical_content_hash(&self, book: BookId) -> Result<Option<ContentHash>, DatabaseError> {
        canonical_hash(&self.connection, book)
    }

    /// Whether this exact revision has been observed for the registered book.
    pub fn book_has_content_revision(&self, book: BookId, checksum: ContentHash) -> Result<bool, DatabaseError> {
        if book.content_hash() == Some(checksum) {
            return Ok(true);
        }
        Ok(self.connection.query_row("SELECT EXISTS(SELECT 1 FROM local_content_alias WHERE book_id=?1 AND byte_hash=?2)", params![book, checksum], |row| row.get(0))?)
    }

    /// Existing live books whose exact bytes can be adopted at a new root.
    pub fn registered_books_for_content(&self, checksum: ContentHash) -> Result<Vec<(BookId, BookFormat)>, DatabaseError> {
        Ok(self
            .connection
            .prepare_cached(
                "SELECT b.content_hash,b.format FROM book b WHERE b.deleted_at IS NULL
            AND (b.content_hash=?1 OR EXISTS(SELECT 1 FROM local_content_alias a WHERE a.book_id=b.content_hash AND a.byte_hash=?2)
                OR EXISTS(SELECT 1 FROM book_content c WHERE c.book_id=b.content_hash AND c.canonical_hash=?2))
            AND EXISTS(SELECT 1 FROM book_dir d WHERE d.book_row_id=b.row_id AND d.deleted_at IS NULL)
            ORDER BY b.content_hash",
            )?
            .query_map(params![BookId::from(checksum), checksum], |row| Ok((row.get::<_, BookId>(0)?, row.get::<_, Option<i64>>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(|(book, format)| format.and_then(BookFormat::from_code).map(|format| (book, format)))
            .collect())
    }

    /// A hash can belong to multiple logical books (particularly provider
    /// files). Ambiguous hashes require a placement or an embedded identity.
    pub fn book_for_local_content(&self, checksum: ContentHash) -> Result<Option<BookId>, DatabaseError> {
        let mut query = self.connection.prepare_cached("SELECT a.book_id FROM local_content_alias a JOIN book b ON b.content_hash=a.book_id WHERE a.byte_hash=?1 AND b.deleted_at IS NULL LIMIT 2")?;
        let books = query.query_map([checksum], |row| row.get(0))?.collect::<Result<Vec<BookId>, _>>()?;
        Ok((books.len() == 1).then(|| books[0]))
    }

    pub fn local_content_hash(&self, book: BookId, directory: DirId) -> Result<Option<ContentHash>, DatabaseError> {
        Ok(self.connection.query_row("SELECT f.byte_hash FROM local_file_content f JOIN book b ON b.content_hash=f.book_id JOIN book_dir d ON d.book_row_id=b.row_id AND d.dir_id=f.dir_id WHERE f.book_id=?1 AND f.dir_id=?2 AND d.deleted_at IS NULL AND d.is_downloaded=1", params![book, directory.to_string()], |row| row.get(0)).optional()?)
    }

    /// Called after an intentional local PDF annotation write. Reader state
    /// continues to use `book`; this write never authors synchronized state.
    pub fn record_local_annotation_export(&self, book: BookId, directory: DirId, checksum: ContentHash, fingerprint: &str, size_bytes: u64) -> Result<(), DatabaseError> {
        self.write(|tx| {
            let updated = tx.execute(
                "UPDATE book_dir SET local_hash=?3,size_bytes=?4,is_downloaded=1 WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND dir_id=?2 AND deleted_at IS NULL",
                params![book, directory.to_string(), fingerprint, i64::try_from(size_bytes).map_err(DatabaseError::operation)?],
            )?;
            if updated != 1 {
                return Err(DatabaseError::message("annotation export placement is no longer present"));
            }
            observe_content(tx, book, directory, checksum)?;
            // A pending first upload follows the edited bytes. Once uploaded,
            // all later revisions stay local and cannot requeue the book.
            if expected_hash(tx, book)?.is_none() {
                use crate::shared_sql::SharedSql;
                tx.shared_scanner_enqueue_upload(&book, i64::try_from(size_bytes).map_err(DatabaseError::operation)?)?;
                tx.execute("UPDATE book_transfer SET upload_checksum=?2,upload_prepared=NULL WHERE content_hash=?1", params![book, checksum])?;
                tx.execute("INSERT OR IGNORE INTO local_book_work(content_hash) VALUES(?1)", [book])?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use book_model::{Annotation, AnnotationAnchor, AnnotationStyle, BookFormat, PdfAnnotationAnchor};
    use sync_common::ROOT_DIR_ID;

    fn hash(bytes: &[u8]) -> ContentHash {
        blake3::hash(bytes).into()
    }

    fn library() -> (Database, BookId) {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let book = BookId::from(hash(b"original PDF"));
        db.connection.execute("INSERT INTO book(content_hash,format,title) VALUES(?1,?2,'Book')", params![book, BookFormat::Pdf as i64]).unwrap();
        db.connection.execute("INSERT INTO book_dir(book_row_id,dir_id,file_name,local_hash,is_downloaded) SELECT row_id,?2,'Book.pdf','first',1 FROM book WHERE content_hash=?1", params![book, ROOT_DIR_ID.to_string()]).unwrap();
        db.connection.execute("INSERT INTO book_transfer(content_hash,upload_id,upload_size,upload_prepared) VALUES(?1,1,100,1)", [book]).unwrap();
        observe_content(&db.connection, book, ROOT_DIR_ID, hash(b"original PDF")).unwrap();
        (db, book)
    }

    #[test]
    fn local_revisions_preserve_book_references_and_freeze_only_uploaded_bytes() {
        let (db, book) = library();
        let annotation = Annotation {
            id: "note".into(),
            book_id: book,
            anchor: AnnotationAnchor::Pdf(PdfAnnotationAnchor::new(0, vec![], None)),
            exact_text: "quote".into(),
            style: AnnotationStyle::Highlight,
            color: 0xffff00,
            note: "my note".into(),
            created_at: 1,
            modified_at: 1,
            toc_ordinal: None,
            progress: None,
            deleted: false,
        };
        db.upsert_annotation(&annotation).unwrap();
        let row: i64 = db.connection.query_row("SELECT row_id FROM book WHERE content_hash=?1", [book], |r| r.get(0)).unwrap();
        let pre_upload = hash(b"PDF with annotations before upload");
        db.record_local_annotation_export(book, ROOT_DIR_ID, pre_upload, "second", 200).unwrap();
        assert_eq!(db.canonical_content_hash(book).unwrap(), None);
        assert_eq!(db.local_content_hash(book, ROOT_DIR_ID).unwrap(), Some(pre_upload));
        assert_eq!(Database::upload_intent_of(&db.connection, &book).unwrap().unwrap().byte_checksum, Some(pre_upload));
        db.confirm_uploaded_content_with_pdf(book, pre_upload, Some(200), None).unwrap();
        let outbox: i64 = db.connection.query_row("SELECT count(*) FROM sync_outbox", [], |r| r.get(0)).unwrap();
        let post_upload = hash(b"PDF with additional local annotations");
        db.record_local_annotation_export(book, ROOT_DIR_ID, post_upload, "third", 300).unwrap();
        assert_eq!(db.canonical_content_hash(book).unwrap(), Some(pre_upload));
        assert_eq!(db.local_content_hash(book, ROOT_DIR_ID).unwrap(), Some(post_upload));
        assert_eq!(db.book_for_local_content(post_upload).unwrap(), Some(book));
        assert_eq!(db.book_for_local_content(pre_upload).unwrap(), Some(book));
        assert!(Database::upload_intent_of(&db.connection, &book).unwrap().is_none());
        assert_eq!(db.annotations(book).unwrap(), vec![annotation]);
        assert_eq!(db.connection.query_row("SELECT row_id FROM book WHERE content_hash=?1", [book], |r| r.get::<_, i64>(0)).unwrap(), row);
        assert_eq!(db.connection.query_row("SELECT count(*) FROM sync_outbox", [], |r| r.get::<_, i64>(0)).unwrap(), outbox);
        assert!(db.confirm_uploaded_content_with_pdf(book, post_upload, None, None).is_err());
        assert_eq!(db.canonical_content_hash(book).unwrap(), Some(pre_upload));
        assert_eq!(db.resolve_book_identity(post_upload.into()).unwrap(), book);
        db.set_asset_storage_policy(true, true).unwrap();
        let queued = Database::upload_intent_of(&db.connection, &book).unwrap().unwrap();
        assert_eq!(queued.byte_checksum, Some(pre_upload));
        assert_eq!(queued.size_bytes, 200);
    }

    #[test]
    fn accepted_upload_remains_canonical_when_local_export_won_the_race() {
        let (db, book) = library();
        let accepted = hash(b"original PDF");
        let newer = hash(b"local export while upload is in flight");
        db.record_local_annotation_export(book, ROOT_DIR_ID, newer, "changed", 300).unwrap();
        db.confirm_uploaded_content_with_pdf(book, accepted, None, None).unwrap();
        assert_eq!(db.canonical_content_hash(book).unwrap(), Some(accepted));
        assert_eq!(db.local_content_hash(book, ROOT_DIR_ID).unwrap(), Some(newer));
        assert!(Database::upload_intent_of(&db.connection, &book).unwrap().is_none());
    }

    #[test]
    fn later_presence_confirmation_preserves_canonical_pdf_metadata_and_length() {
        let (db, book) = library();
        let checksum = hash(b"canonical annotated PDF");
        let pdf = pdf_view_common::PdfReaderMetadata { version: 1, book_id: book.to_hex(), pages: vec![pdf_view_common::PdfStoredPage { width: 600.0, height: 800.0, rotation: 0 }], annotations: vec![] };
        db.confirm_uploaded_content_with_pdf(book, checksum, Some(200), Some(pdf.clone())).unwrap();
        db.confirm_uploaded_content_with_pdf(book, checksum, None, None).unwrap();
        assert_eq!(db.pdf_metadata(&book).unwrap(), Some(pdf.clone()));
        db.record_local_annotation_export(book, ROOT_DIR_ID, hash(b"new local PDF"), "edited", 300).unwrap();
        assert_eq!(db.local_pdf_metadata(&book, "/Book.pdf").unwrap(), None);
        assert_eq!(db.pdf_metadata(&book).unwrap(), Some(pdf));
        let size: Option<i64> = db.connection.query_row("SELECT size_bytes FROM book_content WHERE book_id=?1", [book], |row| row.get(0)).unwrap();
        assert_eq!(size, Some(200));
        assert!(db.confirm_uploaded_content_with_pdf(book, checksum, Some(300), None).is_ok());
        // The already confirmed canonical length wins over local observations.
        let size: Option<i64> = db.connection.query_row("SELECT size_bytes FROM book_content WHERE book_id=?1", [book], |row| row.get(0)).unwrap();
        assert_eq!(size, Some(200));
    }

    #[test]
    fn recovered_pdf_provenance_constrains_upload_without_freezing_unverified_bytes() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        let book = BookId::for_local_content(&[9; 16], hash(b"initial PDF"));
        let original = hash(b"original server revision");
        let edited = hash(b"edited local PDF");
        let source = book_inspection::PdfSourceIdentity { book_id: book, canonical_hash: Some(original) };
        assert_eq!(db.resolve_scanned_content(edited, Some(source)).unwrap(), book);
        db.commit_import(crate::ImportCommit {
            source_identity: Some(source),
            byte_checksum: Some(edited),
            parent_id: ROOT_DIR_ID,
            file_name: "Book.pdf".into(),
            format: BookFormat::Pdf,
            book_id: book,
            size_bytes: 400,
            inspection_version: 1,
            inspection: Some(book_inspection::InspectedBook { metadata: book_model::BookRecord { metadata: Default::default(), description: String::new() }, pdf: None, audiobook: None, toc: vec![] }),
            published: library_model::PublishedStagedBook { name: "Book.pdf".into(), relative_path: "/Book.pdf".into(), published: true, fingerprint: Some("edited".into()) },
            request_thumbnail: false,
            restore_paths: vec![],
        })
        .unwrap();
        assert_eq!(db.canonical_content_hash(book).unwrap(), None);
        assert_eq!(db.local_content_hash(book, ROOT_DIR_ID).unwrap(), Some(edited));
        assert_eq!(Database::upload_intent_of(&db.connection, &book).unwrap().unwrap().byte_checksum, Some(original));
        db.record_local_annotation_export(book, ROOT_DIR_ID, hash(b"newer export"), "newer", 500).unwrap();
        assert_eq!(Database::upload_intent_of(&db.connection, &book).unwrap().unwrap().byte_checksum, Some(original));
        db.set_asset_storage_policy(true, true).unwrap();
        assert_eq!(Database::upload_intent_of(&db.connection, &book).unwrap().unwrap().byte_checksum, Some(original));
        // A marker is not authoritative. Verified server state can replace a
        // stale hint, after which the confirmed checksum cannot change.
        let verified = hash(b"verified canonical revision");
        db.confirm_uploaded_content_with_pdf(book, verified, Some(200), None).unwrap();
        assert_eq!(db.canonical_content_hash(book).unwrap(), Some(verified));
        assert_eq!(expected_hash(&db.connection, book).unwrap(), Some(verified));
        assert!(db.confirm_uploaded_content_with_pdf(book, original, None, None).is_err());
    }

    #[test]
    fn ambiguous_aliases_and_conflicting_markers_do_not_merge_books() {
        let (db, book) = library();
        let shared = hash(b"same bytes, two identities");
        record_alias(&db.connection, book, shared).unwrap();
        let other = BookId::for_local_content(&[9; 16], blake3::hash(b"separate book").into());
        db.connection.execute("INSERT INTO book(content_hash) VALUES(?1)", [other]).unwrap();
        record_alias(&db.connection, other, shared).unwrap();
        assert_eq!(db.book_for_local_content(shared).unwrap(), None);
        let canonical = hash(b"original PDF");
        db.confirm_uploaded_content_with_pdf(book, canonical, Some(100), None).unwrap();
        let source = book_inspection::PdfSourceIdentity { book_id: book, canonical_hash: Some(hash(b"wrong source")) };
        let modified = hash(b"a different PDF");
        assert_eq!(db.resolve_scanned_content(modified, Some(source)).unwrap(), BookId::for_local_content(&identity_scope(&db.connection).unwrap(), modified));
        let source = book_inspection::PdfSourceIdentity { canonical_hash: Some(canonical), ..source };
        assert_eq!(db.resolve_scanned_content(modified, Some(source)).unwrap(), book);
    }
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn cached_import_source(path: &str, fingerprint: u64) -> Option<ContentHash>;
    arguments (String, u64,) = (path.to_owned(), fingerprint,);
    execute database, arguments => database.cached_import_source(&arguments.0, arguments.1);

    fn local_content_revision(book: BookId, directory: DirId, checksum: ContentHash) -> Option<String>;
    arguments (BookId, DirId, ContentHash,) = (book, directory, checksum,);
    execute database, arguments => database.local_content_revision(arguments.0, arguments.1, arguments.2);

    fn resolve_book_identity(reference: BookId) -> BookId;
    arguments (BookId,) = (reference,);
    execute database, arguments => database.resolve_book_identity(arguments.0);

    fn resolve_scanned_content(checksum: ContentHash, source: Option<book_inspection::PdfSourceIdentity>) -> BookId;
    arguments (ContentHash, Option<book_inspection::PdfSourceIdentity>,) = (checksum, source,);
    execute database, arguments => database.resolve_scanned_content(arguments.0, arguments.1);

    fn confirm_uploaded_content_with_pdf(book: BookId, checksum: ContentHash, size_bytes: Option<u64>, pdf: Option<pdf_view_common::PdfReaderMetadata>) -> ();
    arguments (BookId, ContentHash, Option<u64>, Option<pdf_view_common::PdfReaderMetadata>,) = (book, checksum, size_bytes, pdf,);
    execute database, arguments => database.confirm_uploaded_content_with_pdf(arguments.0, arguments.1, arguments.2, arguments.3);

    fn confirm_downloaded_content(book: BookId, checksum: ContentHash) -> ();
    arguments (BookId, ContentHash,) = (book, checksum,);
    execute database, arguments => database.confirm_downloaded_content(arguments.0, arguments.1);

    fn canonical_content_hash(book: BookId) -> Option<ContentHash>;
    arguments (BookId,) = (book,);
    execute database, arguments => database.canonical_content_hash(arguments.0);

    fn book_has_content_revision(book: BookId, checksum: ContentHash) -> bool;
    arguments (BookId, ContentHash,) = (book, checksum,);
    execute database, arguments => database.book_has_content_revision(arguments.0, arguments.1);

    fn registered_books_for_content(checksum: ContentHash) -> Vec<(BookId, BookFormat)>;
    arguments (ContentHash,) = (checksum,);
    execute database, arguments => database.registered_books_for_content(arguments.0);

    fn book_for_local_content(checksum: ContentHash) -> Option<BookId>;
    arguments (ContentHash,) = (checksum,);
    execute database, arguments => database.book_for_local_content(arguments.0);

    fn local_content_hash(book: BookId, directory: DirId) -> Option<ContentHash>;
    arguments (BookId, DirId,) = (book, directory,);
    execute database, arguments => database.local_content_hash(arguments.0, arguments.1);

    fn record_local_annotation_export(book: BookId, directory: DirId, checksum: ContentHash, fingerprint: &str, size_bytes: u64) -> ();
    arguments (BookId, DirId, ContentHash, String, u64,) = (book, directory, checksum, fingerprint.to_owned(), size_bytes,);
    execute database, arguments => database.record_local_annotation_export(arguments.0, arguments.1, arguments.2, &arguments.3, arguments.4);
}
