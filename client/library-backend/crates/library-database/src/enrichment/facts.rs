//! Optional enrichment updates records that are already publishable.
use crate::query_methods::{blob_column, first_column, query_rows};
use crate::sync::apply::registers::{canonical_body, record_local};
use crate::{Database, DatabaseError};
use book_model::{BookFormat, BookRecord};
use include_sqlite_sql::include_sql;
use library_replica::{EnrichmentResult, LibraryStateEntry};
use metadata_contract::books::{BookEnrichment, BookEnrichmentStatus};
use rusqlite::{types::Type, Connection, Error as SqlError, OptionalExtension};
use sync_common::{BookId, StateKey};

/// What the device stores about a book the metadata server hasn't answered.
#[derive(Clone, Debug)]
pub struct EnrichmentCandidate {
    pub book_id: BookId,
    pub request_key: String,
    /// `description` is empty: the file's own description always wins.
    pub record: BookRecord,
    pub needs_description: bool,
    pub audiobook: bool,
    pub recording: Option<metadata_contract::audible::LookupRequest>,
    /// Local, never synchronized.
    pub filenames: Vec<String>,
}

/// The current record identifies the exact input of an optional lookup.
fn facts_input(connection: &Connection, hash: &BookId) -> Result<Option<Vec<u8>>, DatabaseError> {
    Ok(first_column(|read| connection.facts_metadata_body(hash, read))?)
}

fn request_key(body: &[u8]) -> String {
    book_model::agent_id_from_migration_seed(body).to_string()
}

impl Database {
    /// A bounded page of due lookups and its last scanned book, including corrupt inputs.
    /// Successful answers and definite misses do not expire.
    pub fn enrichment_candidates(&self, now: i64, after: Option<&BookId>) -> Result<(Vec<EnrichmentCandidate>, Option<BookId>), DatabaseError> {
        self.enrichment_candidates_excluding(now, after, &[])
    }

    /// Newly arrived books can be dispatched while earlier requests are pending.
    pub fn enrichment_candidates_excluding(&self, now: i64, after: Option<&BookId>, excluded: &[BookId]) -> Result<(Vec<EnrichmentCandidate>, Option<BookId>), DatabaseError> {
        let mut candidates = Vec::new();
        let mut last = None;
        let excluded = serde_json::to_string(excluded).map_err(DatabaseError::operation)?;
        self.connection.facts_candidates(now, after, &excluded, |row| {
            let hash = row.get(0)?;
            last = Some(hash);
            let format: Option<i64> = row.get(1)?;
            let body = blob_column(row, 2)?;
            let key = request_key(body);
            let value = match sync_common::wire::decode::<LibraryStateEntry>(body, sync_common::wire::MAX_DECODED_REQUEST_BYTES) {
                Ok(LibraryStateEntry::BookAdd { value, .. }) => value,
                result => {
                    log::warn!("ignoring invalid enrichment input for {hash}: {result:?}");
                    return Ok(());
                }
            };
            let recording = value.recording;
            let audiobook = matches!(format.and_then(BookFormat::from_code), Some(BookFormat::M4b | BookFormat::Mp3Folder));
            let needs_description = value.record.description.trim().is_empty();
            let record = BookRecord { metadata: value.record.metadata, description: String::new() };
            candidates.push(EnrichmentCandidate { book_id: hash, request_key: key, record, needs_description, audiobook, recording, filenames: Vec::new() });
            Ok(())
        })?;
        if candidates.is_empty() {
            return Ok((candidates, last));
        }
        let hashes = candidates.iter().map(|candidate| candidate.book_id).collect::<Vec<_>>();
        let requested = serde_json::to_string(&hashes).map_err(DatabaseError::operation)?;
        self.connection.facts_filenames_batch(&requested, |row| {
            let index = usize::try_from(row.get::<_, i64>(0)?).map_err(|error| SqlError::FromSqlConversionFailure(0, Type::Integer, Box::new(error)))?;
            candidates[index].filenames.push(row.get(1)?);
            Ok(())
        })?;
        Ok((candidates, last))
    }

    /// Apply optional results to the current record, without holding up file
    /// publication or rolling other books back for an invalid provider result.
    pub fn record_enrichments(&self, results: impl IntoIterator<Item = (BookId, String, BookEnrichment)>, now: i64) -> Result<bool, DatabaseError> {
        self.write(|tx| {
            let mut changed = false;
            for (hash, key, result) in results {
                let Some(bytes) = facts_input(tx, &hash)? else { continue };
                if key != request_key(&bytes) {
                    continue;
                }
                let LibraryStateEntry::BookAdd { value: mut addition, .. } = sync_common::wire::decode(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? else { continue };
                let original = addition.clone();
                let mut retry = result.status == BookEnrichmentStatus::Failed || result.failure.is_some();
                match result.status {
                    BookEnrichmentStatus::Found => {
                        if let Some(record) = result.record {
                            addition.apply_enrichment(record, EnrichmentResult::Found { toc: result.toc, cover_id: result.cover.map(|cover| cover.id) });
                        } else {
                            retry = true;
                        }
                    }
                    BookEnrichmentStatus::NoMatch if addition.enrichment.is_none() => addition.enrichment = Some(EnrichmentResult::NoMatch),
                    BookEnrichmentStatus::NoMatch => {}
                    BookEnrichmentStatus::Failed => {}
                }
                let updated = addition != original;
                let value = LibraryStateEntry::BookAdd { book_id: hash, value: addition };
                if let Err(error) = value.validate() {
                    log::warn!("ignoring invalid enrichment for {hash}: {error}");
                    retry = true;
                } else if updated {
                    record_local(tx, value)?;
                    changed = true;
                }
                tx.execute(
                    "INSERT INTO book_transfer(content_hash,enrichment_retry_at) VALUES(?1,?2)
                    ON CONFLICT(content_hash) DO UPDATE SET enrichment_retry_at=excluded.enrichment_retry_at",
                    rusqlite::params![hash, retry.then(|| now.saturating_add(60))],
                )?;
            }
            Ok(changed)
        })
    }

    /// Records a catalog book's record as its answer, so the metadata server
    /// is not asked about it. The catalog already enriched and curated it.
    /// Does nothing once the book has an answer.
    pub fn record_catalog_answer(&self, hash: &BookId, record: BookRecord) -> Result<(), DatabaseError> {
        self.write(|transaction| record_catalog_answer(transaction, hash, record))
    }

    /// Answered books in `scope` still eligible for direct cover lookup.
    pub fn enrichment_cover_candidates(&self, after: Option<&BookId>) -> Result<Vec<BookId>, DatabaseError> {
        Ok(query_rows(|read| self.connection.facts_cover_candidates(after, read), |row| row.get::<_, BookId>(0))?)
    }

    /// The finalized record used by the library projection.
    /// A definitive metadata miss still permits a direct cover-provider lookup.
    pub fn enrichment_cover_record(&self, hash: &BookId) -> Result<Option<BookRecord>, DatabaseError> {
        let Some((bytes, _)) = canonical_body(&self.connection, "book_add", &StateKey::Book(*hash), "")? else { return Ok(None) };
        let LibraryStateEntry::BookAdd { value, .. } = sync_common::wire::decode::<LibraryStateEntry>(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? else {
            return Err(DatabaseError::message("invalid stored book addition"));
        };
        let mut record = value.record;
        record.description.clear();
        Ok(Some(record))
    }

    /// The earliest retry time after a live book's enrichment request failed.
    pub fn enrichment_next_retry(&self) -> Result<Option<i64>, DatabaseError> {
        Ok(self.connection.facts_next_retry(|row| row.get::<_, Option<i64>>(0))?)
    }

    pub fn mark_enrichment_cover_checked(&self, hash: &BookId) -> Result<(), DatabaseError> {
        self.connection.facts_mark_cover_checked(hash)?;
        Ok(())
    }

    pub fn enrichment_cover_input(&self, hash: &BookId) -> Result<Option<(Option<BookRecord>, Option<String>, Option<Vec<u8>>)>, DatabaseError> {
        let input: Option<(Option<Vec<u8>>, Option<String>)> = self.connection.facts_cover_input(hash, |row| Ok((row.get(0)?, row.get(1)?))).optional()?;
        let Some((body, expected)) = input else { return Ok(None) };
        let record = match body.as_deref() {
            Some(bytes) => {
                let LibraryStateEntry::BookAdd { value, .. } = sync_common::wire::decode(bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? else {
                    return Err(DatabaseError::message("invalid stored book addition"));
                };
                let mut record = value.record;
                record.description.clear();
                Some(record)
            }
            None => None,
        };
        Ok(Some((record, expected, body)))
    }

    /// A changed record retains its own pending cover lookup.
    pub fn mark_enrichment_cover_checked_if_current(&self, hash: &BookId, body: Option<&[u8]>) -> Result<(), DatabaseError> {
        self.connection.facts_mark_cover_checked_if_current(hash, body)?;
        Ok(())
    }
}

include_sql!("src/enrichment/sql/facts.sql");

/// Completes an import's catalog answer under the caller's writer transaction.
pub(crate) fn record_catalog_answer(transaction: &rusqlite::Transaction<'_>, hash: &BookId, record: BookRecord) -> Result<(), DatabaseError> {
    let Some(body) = facts_input(transaction, hash)? else { return Ok(()) };
    let LibraryStateEntry::BookAdd { value: mut addition, .. } = sync_common::wire::decode::<LibraryStateEntry>(&body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? else {
        return Err(DatabaseError::message("invalid book record"));
    };
    if addition.enrichment.is_some() {
        return Ok(());
    }
    addition.apply_enrichment(record, EnrichmentResult::Found { toc: None, cover_id: None });
    record_local(transaction, LibraryStateEntry::BookAdd { book_id: *hash, value: addition })?;
    transaction.execute("UPDATE book_transfer SET enrichment_retry_at=NULL WHERE content_hash=?1", [hash])?;
    Ok(())
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn record_enrichments(answers: Vec<(BookId, String, BookEnrichment)>, now: i64) -> bool;
    arguments (Vec<(BookId, String, BookEnrichment)>, i64) = (answers, now);
    execute database, arguments => database.record_enrichments(arguments.0, arguments.1);

    fn enrichment_candidates(now: i64, after: Option<&BookId>) -> (Vec<EnrichmentCandidate>, Option<BookId>);
    arguments (i64, Option<BookId>) = (now, after.copied());
    execute database, arguments => database.enrichment_candidates(arguments.0, arguments.1.as_ref());

    fn enrichment_candidates_excluding(now: i64, after: Option<&BookId>, excluded: &[BookId]) -> (Vec<EnrichmentCandidate>, Option<BookId>);
    arguments (i64, Option<BookId>, Vec<BookId>) = (now, after.copied(), excluded.to_vec());
    execute database, arguments => database.enrichment_candidates_excluding(arguments.0, arguments.1.as_ref(), &arguments.2);

    fn record_catalog_answer(hash: &BookId, record: BookRecord) -> ();
    arguments (BookId, BookRecord,) = (hash.clone(), record,);
    execute database, arguments => database.record_catalog_answer(&arguments.0, arguments.1);

    fn enrichment_cover_candidates(after: Option<&BookId>) -> Vec<BookId>;
    arguments (Option<BookId>,) = (after.copied(),);
    execute database, arguments => database.enrichment_cover_candidates(arguments.0.as_ref());

    fn enrichment_cover_record(hash: &BookId) -> Option<BookRecord>;
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.enrichment_cover_record(&arguments.0);

    fn enrichment_next_retry() -> Option<i64>;
    arguments () = ();
    execute database, arguments => database.enrichment_next_retry();

    fn mark_enrichment_cover_checked(hash: &BookId) -> ();
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.mark_enrichment_cover_checked(&arguments.0);

    fn enrichment_cover_input(hash: &BookId) -> Option<(Option<BookRecord>, Option<String>, Option<Vec<u8>>)>;
    arguments (BookId,) = (*hash,);
    execute database, arguments => database.enrichment_cover_input(&arguments.0);

    fn mark_enrichment_cover_checked_if_current(hash: &BookId, body: Option<&[u8]>) -> ();
    arguments (BookId, Option<Vec<u8>>) = (*hash, body.map(<[u8]>::to_vec));
    execute database, arguments => database.mark_enrichment_cover_checked_if_current(&arguments.0, arguments.1.as_deref());
}
