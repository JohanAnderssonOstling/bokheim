//! OPDS metadata and local availability. Feed browsing never inspects book files.
use crate::native::observations::{self, Revision};
use crate::sync::apply::registers::record_local;
use crate::{Database, DatabaseError};
use book_model::{AUTHOR_MARC_RELATOR_CODE, BookDetails, BookFormat, BookRecord, Contributor, normalize_search_text};
use content_address::ContentHash;
use library_model::{OpdsPublication, PublishedStagedBook};
use library_replica::{BookAddition, BookFacts, BookLifecycleState, EnrichmentResult, LibraryStateEntry};
use rusqlite::{OptionalExtension, params};
use sync_common::{BookId, ROOT_DIR_ID};

impl Database {
    pub fn configure_opds(&self) -> Result<(), DatabaseError> {
        self.connection.execute_batch("CREATE TABLE IF NOT EXISTS opds_book(book_id BLOB PRIMARY KEY REFERENCES book(content_hash) ON DELETE CASCADE, publication TEXT NOT NULL) STRICT;")?;
        Ok(())
    }

    pub fn register_opds_books(&self, books: &[OpdsPublication], synchronize: bool) -> Result<bool, DatabaseError> {
        self.write(|tx| {
            let mut changed = false;
            for book in books {
                let Some(format) = book.format.filter(|_| book.download_url.is_some()) else { continue };
                let id = book.book_id();
                let author = book.authors.join(", ");
                tx.execute("INSERT INTO book(content_hash,format,title,subtitle,author,search_title,search_author,added_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(content_hash) DO UPDATE SET format=CASE WHEN EXISTS(SELECT 1 FROM book_dir WHERE book_row_id=book.row_id AND is_downloaded=1) THEN book.format ELSE excluded.format END,title=excluded.title,subtitle=excluded.subtitle,author=excluded.author,search_title=excluded.search_title,search_author=excluded.search_author", params![id, format as i64, book.title, book.subtitle, author, normalize_search_text(&book.title), normalize_search_text(&author), crate::sync::unix_millis()?])?;
                tx.execute("INSERT INTO opds_book VALUES(?1,?2) ON CONFLICT(book_id) DO UPDATE SET publication=excluded.publication", params![id, serde_json::to_string(book).map_err(DatabaseError::operation)?])?;
                if synchronize {
                    let has_addition: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sync_book_record WHERE state_kind='book_add' AND content_hash=?1)", [id], |row| row.get(0))?;
                    if !has_addition {
                        let added_at: i64 = tx.query_row("SELECT added_at FROM book WHERE content_hash=?1", [id], |row| row.get(0))?;
                        let contributors = book.authors.iter().filter_map(|author| Contributor::with_id(uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, author.as_bytes()), author, AUTHOR_MARC_RELATOR_CODE).ok()).collect();
                        record_local(tx, LibraryStateEntry::BookLifecycle { book_id: id, value: BookLifecycleState::Present })?;
                        record_local(tx, LibraryStateEntry::BookAdd {
                            book_id: id,
                            value: BookAddition {
                                facts: BookFacts { added_at: u64::try_from(added_at).map_err(DatabaseError::operation)?, format },
                                record: BookRecord { metadata: BookDetails { title: book.title.clone(), subtitle: book.subtitle.clone(), contributors, ..BookDetails::default() }, description: book.description.clone() },
                                navigation: None,
                                pdf: None,
                                recording: None,
                                enrichment: Some(EnrichmentResult::NoMatch),
                            },
                        })?;
                        changed = true;
                    }
                }
                let name = format!("{id}.{}", format.canonical_extension());
                let placed: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND deleted_at IS NULL)", [id], |row| row.get(0))?;
                if !placed { crate::projection::place_book(tx, &ROOT_DIR_ID, &id, &name)?; }
            }
            Ok(changed)
        })
    }

    pub fn opds_publication(&self, id: BookId) -> Result<Option<OpdsPublication>, DatabaseError> {
        // The OPDS table itself identifies a configured source; ordinary
        // libraries can also encounter OPDS identities without having it.
        let configured: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name='opds_book')", [], |row| row.get(0))?;
        if !configured {
            return Ok(None);
        }
        let json: Option<String> = self.connection.query_row("SELECT publication FROM opds_book WHERE book_id=?1", [id], |row| row.get(0)).optional()?;
        json.map(|json| serde_json::from_str(&json).map_err(DatabaseError::operation)).transpose()
    }

    pub fn opds_progress(&self, id: BookId) -> Result<f32, DatabaseError> {
        Ok(self.connection.query_row("SELECT COALESCE(read_progress,0) FROM book WHERE content_hash=?1", [id], |row| row.get(0))?)
    }

    pub fn record_opds_download(&self, id: BookId, published: &PublishedStagedBook, checksum: ContentHash, size: u64, format: BookFormat) -> Result<(), DatabaseError> {
        self.write(|tx| {
            tx.execute("UPDATE book SET format=?2 WHERE content_hash=?1", params![id, format as i64])?;
            tx.execute("UPDATE book_dir SET file_name=?2 WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND dir_id=?3", params![id, published.name, ROOT_DIR_ID.to_string()])?;
            crate::content_identity::observe_content(tx, id, ROOT_DIR_ID, checksum)?;
            observations::record_file(tx, &ROOT_DIR_ID.to_string(), &id, &published.relative_path, published.fingerprint.as_deref(), Revision::Remote { size: i64::try_from(size).map_err(DatabaseError::operation)? })
        })
    }
}
