//! Provider thumbnail publication and device-local cache generations.

use crate::sync::apply::registers::{canonical_body, record_local};
use crate::{Database, DatabaseError};
use content_address::BookId;
use library_replica::LibraryStateEntry;
use rusqlite::{params, Connection, OptionalExtension};
use sync_common::{
    storage::{ProviderFile, ProviderThumbnails},
    StateKey,
};

fn source(file: &ProviderFile, thumbnails: Option<&ProviderThumbnails>) -> Result<String, DatabaseError> {
    serde_json::to_string(&(file, thumbnails)).map_err(DatabaseError::operation)
}

fn current_source(connection: &Connection, book: &BookId) -> Result<Option<String>, DatabaseError> {
    let row: Option<(String, Option<String>)> = connection.query_row("SELECT file,thumbnails FROM provider_book WHERE book_id=?1", [book], |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
    row.map(|(file, thumbnails)| {
        let file: ProviderFile = serde_json::from_str(&file).map_err(DatabaseError::operation)?;
        let thumbnails: Option<ProviderThumbnails> = thumbnails.map(|json| serde_json::from_str(&json)).transpose().map_err(DatabaseError::operation)?;
        source(&file, thumbnails.as_ref())
    })
    .transpose()
}

impl Database {
    /// None means not inspected for the current provider revision/reference pair.
    pub fn provider_cover_cached(&self, book: &BookId) -> Result<Option<bool>, DatabaseError> {
        let Some(current) = current_source(&self.connection, book)? else { return Ok(None) };
        self.connection.query_row("SELECT has_cover FROM provider_cover_cache WHERE book_id=?1 AND source=?2", params![book, current], |row| row.get(0)).optional().map_err(Into::into)
    }

    /// Reject a completion that raced with a provider revision or a new cover.
    pub fn complete_provider_cover_cache(&self, file: &ProviderFile, thumbnails: Option<&ProviderThumbnails>, has_cover: bool) -> Result<bool, DatabaseError> {
        let book = file.identity.book_id();
        let expected = source(file, thumbnails)?;
        self.write(|tx| {
            if current_source(tx, &book)?.as_ref() != Some(&expected) {
                return Ok(false);
            }
            tx.execute("INSERT INTO provider_cover_cache(book_id,source,has_cover) VALUES(?1,?2,?3) ON CONFLICT(book_id) DO UPDATE SET source=excluded.source,has_cover=excluded.has_cover", params![book, expected, has_cover])?;
            Ok(true)
        })
    }

    /// The caller supplies only a complete, confirmed pair. Publication changes
    /// the canonical metadata register in one transaction and preserves facts.
    pub fn publish_provider_thumbnails(&self, file: &ProviderFile, thumbnails: ProviderThumbnails) -> Result<bool, DatabaseError> {
        thumbnails.validate_for(&file.identity.scope).map_err(DatabaseError::operation)?;
        let storage = self.storage_policy()?;
        if storage.external().is_none_or(|root| root.scope != file.identity.scope) {
            return Err(DatabaseError::message("thumbnail belongs to another storage provider"));
        }
        let book = file.identity.book_id();
        self.write(|tx| {
            let Some((body, _)) = canonical_body(tx, "external_book", &StateKey::Book(book), "")? else { return Ok(false) };
            let state: LibraryStateEntry = sync_common::wire::decode(&body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
            let LibraryStateEntry::ExternalBook { mut value, .. } = state else { return Err(DatabaseError::message("invalid provider book state")) };
            if &value.file != file {
                return Ok(false);
            }
            // Another device's completed pair takes precedence over duplicate
            // generation; don't repeatedly replace it with identical artwork.
            if value.thumbnails.is_some() {
                return Ok(false);
            }
            value.thumbnails = Some(thumbnails);
            record_local(tx, LibraryStateEntry::ExternalBook { content_hash: book, value })?;
            Ok(true)
        })
    }
}
