//! Disposable, device-local content search. Canonical book text stays in its file.
mod fts;
use crate::query_methods::invalid_integer_column;
use crate::{Database, DatabaseError, browse::options::BrowseOptions};
use book_model::{BookFormat, BookTextCheckpoint, BookTextIndex, BookTextLocation};
use library_model::{BookTextMatchPage, NotebookSort};
use rusqlite::{OptionalExtension, params};
use sync_common::BookId;

pub struct TextIndexBook {
    pub book: BookId,
    pub format: BookFormat,
    pub revision: String,
}

/// Owned preparation for one visible window of a book's search matches.
pub struct TextMatchPage {
    pub format: BookFormat,
    pub revision: String,
    pub locations: Vec<BookTextLocation>,
    pub page: BookTextMatchPage,
}

const SOURCE: &str = "SELECT b.row_id,b.content_hash,b.format,
    'v1:' || COALESCE((SELECT group_concat(value,':') FROM (SELECT hex(byte_hash) AS value FROM local_file_content WHERE book_id=b.content_hash ORDER BY dir_id)),
    (SELECT hex(canonical_hash) FROM book_content WHERE book_id=b.content_hash),hex(b.content_hash)) AS revision
    FROM book b JOIN library_book_presence p ON p.book_row_id=b.row_id
    WHERE p.downloaded=1 AND b.deleted_at IS NULL AND b.format IN (1,2,4,32)";

impl Database {
    fn text_index_installed(&self) -> Result<bool, DatabaseError> {
        Ok(self.connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='book_text_state')", [], |row| row.get(0))?)
    }

    pub fn configure_text_index(&self, enabled: bool) -> Result<(), DatabaseError> {
        if enabled {
            if !self.text_index_installed()? {
                self.write(|tx| {
                    tx.execute_batch(include_str!("schema.sql"))?;
                    Ok(())
                })?;
                self.browse_results.replace(Default::default());
            }
        } else {
            if self.text_index_installed()? {
                self.text_index_usage.borrow_mut().take();
                self.write(|tx| {
                    // Survives a failed VACUUM and a process restart.
                    tx.execute_batch("CREATE TABLE IF NOT EXISTS book_text_cleanup(singleton INTEGER PRIMARY KEY CHECK(singleton=1));")?;
                    let triggers = tx.prepare("SELECT name FROM sqlite_schema WHERE type='trigger' AND name GLOB 'book_text_*'")?.query_map([], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
                    for trigger in triggers {
                        tx.execute_batch(&format!("DROP TRIGGER {trigger}"))?;
                    }
                    tx.execute_batch("DROP TABLE book_text_state; DROP TABLE book_text; DROP TABLE book_text_revision;")?;
                    Ok(())
                })?;
                self.browse_results.replace(Default::default());
            }
            let pending: bool = self.connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='book_text_cleanup')", [], |row| row.get(0))?;
            if pending {
                self.connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM main; DROP TABLE book_text_cleanup;")?;
            }
        }
        Ok(())
    }

    pub(crate) fn text_index_revision(&self) -> Result<(bool, i64), DatabaseError> {
        let enabled = client_platform_runtime::text_search::enabled();
        let revision = if self.text_index_installed()? { self.connection.query_row("SELECT revision FROM book_text_revision WHERE singleton=1", [], |row| row.get(0))? } else { 0 };
        Ok((enabled, revision))
    }

    pub fn text_index_bytes(&self) -> Result<u64, DatabaseError> {
        if !self.text_index_installed()? {
            self.text_index_usage.borrow_mut().take();
            return Ok(0);
        }
        let revision = self.connection.query_row("SELECT revision FROM book_text_revision WHERE singleton=1", [], |row| row.get::<_, i64>(0))?;
        if let Some((cached, bytes)) = *self.text_index_usage.borrow() {
            if cached == revision {
                return Ok(bytes);
            }
        }
        let bytes = self.connection.query_row("SELECT COALESCE(SUM(pgsize),0) FROM dbstat WHERE name IN ('book_text_data','book_text_idx','book_text_docsize','book_text_config','book_text_state','book_text_revision')", [], |row| {
            u64::try_from(row.get::<_, i64>(0)?).map_err(|error| invalid_integer_column(0, error.to_string()))
        })?;
        self.text_index_usage.replace(Some((revision, bytes)));
        Ok(bytes)
    }

    pub fn text_index_candidates(&self) -> Result<Vec<TextIndexBook>, DatabaseError> {
        let sql = format!(
            "WITH sources AS ({SOURCE}) SELECT content_hash,format,revision FROM sources s
            WHERE NOT EXISTS(SELECT 1 FROM book_text_state i WHERE i.row_id=s.row_id AND i.revision=s.revision) ORDER BY row_id"
        );
        Ok(self
            .connection
            .prepare(&sql)?
            .query_map([], |row| {
                let code: i64 = row.get(1)?;
                Ok(TextIndexBook { book: row.get(0)?, format: BookFormat::from_code(code).ok_or(rusqlite::Error::InvalidQuery)?, revision: row.get(2)? })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }

    pub fn prune_text_index(&self) -> Result<(), DatabaseError> {
        self.write(|tx| {
            tx.execute(&format!("WITH sources AS ({SOURCE}) DELETE FROM book_text_state WHERE NOT EXISTS(SELECT 1 FROM sources s WHERE s.row_id=book_text_state.row_id AND s.revision=book_text_state.revision)"), [])?;
            Ok(())
        })
    }

    pub fn remove_text_match(&self, book: BookId, revision: &str) -> Result<(), DatabaseError> {
        if !self.text_index_installed()? {
            return Ok(());
        }
        self.write(|tx| {
            tx.execute("DELETE FROM book_text_state WHERE row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND revision=?2", params![book, revision])?;
            Ok(())
        })
    }

    pub async fn store_book_text_batch(&self, batch: Vec<(TextIndexBook, BookTextIndex)>) -> Result<(), DatabaseError> {
        if batch.is_empty() {
            return Ok(());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = self.path.clone();
            client_platform_runtime::executor::run_blocking(move || Self::open(path)?.write_book_text_batch(batch)).await.map_err(DatabaseError::operation)?
        }
        #[cfg(target_arch = "wasm32")]
        self.write_book_text_batch(batch)
    }

    fn write_book_text_batch(&self, batch: Vec<(TextIndexBook, BookTextIndex)>) -> Result<(), DatabaseError> {
        let batch = batch.into_iter().map(|(book, index)| serde_json::to_string(&index.checkpoints).map(|checkpoints| (book, index.text, checkpoints)).map_err(DatabaseError::operation)).collect::<Result<Vec<_>, _>>()?;
        self.write(|tx| {
            if !client_platform_runtime::text_search::enabled() {
                return Ok(());
            }
            let mut source = tx.prepare(&format!("SELECT row_id FROM ({SOURCE}) WHERE content_hash=?1 AND revision=?2"))?;
            let mut remove = tx.prepare("DELETE FROM book_text_state WHERE row_id=?1")?;
            let mut insert_text = tx.prepare("INSERT INTO book_text(rowid,body) VALUES(?1,?2)")?;
            let mut insert_state = tx.prepare("INSERT INTO book_text_state VALUES(?1,?2,?3)")?;
            for (book, text, checkpoints) in batch {
                // Recheck each source under the writer lock; stale books are skipped.
                let row = source.query_row(params![book.book, book.revision], |row| row.get::<_, i64>(0)).optional()?;
                let Some(row) = row else { continue };
                remove.execute([row])?;
                insert_text.execute(params![row, text])?;
                insert_state.execute(params![row, book.revision, checkpoints])?;
            }
            Ok(())
        })
    }

    pub(crate) fn text_search_ids(&self, options: &BrowseOptions, sort: NotebookSort) -> Result<Vec<(i64, Box<[u32]>)>, DatabaseError> {
        if options.search.trim().chars().take(3).count() < 3 || !client_platform_runtime::text_search::enabled() || !self.text_index_installed()? {
            return Ok(Vec::new());
        }
        let tokens = fts::tokens(&self.connection, &options.search)?;
        if tokens.is_empty() {
            return Ok(Vec::new());
        }
        // Quote every normalized token; users never supply FTS operators.
        // The final word is a prefix, while preceding words remain exact and adjacent.
        let phrase = tokens.iter().map(|token| format!("\"{}\"", token.replace('"', "\"\""))).collect::<Vec<_>>().join(" + ") + "*";
        let order = if sort == NotebookSort::BookName { "b.search_title,b.row_id" } else { "b.added_at DESC,b.row_id" };
        let sql = format!(
            "SELECT b.row_id,book_matches(book_text) FROM book_text
            JOIN book b ON b.row_id=book_text.rowid
            WHERE book_text MATCH ?1 AND (?2=0 OR (b.format & ?2)!=0) ORDER BY {order}"
        );
        Ok(self
            .connection
            .prepare(&sql)?
            .query_map(params![phrase, options.file_types], |row| {
                let bytes = row.get::<_, Vec<u8>>(1)?;
                let offsets = bytes.chunks_exact(4).map(|bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])).collect();
                Ok((row.get(0)?, offsets))
            })?
            .collect::<Result<Vec<_>, _>>()?)
    }
}

pub(crate) fn register(connection: &rusqlite::Connection) -> rusqlite::Result<()> {
    fts::register(connection)
}
