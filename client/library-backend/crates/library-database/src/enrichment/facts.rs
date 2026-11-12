//! Enrich local drafts once, then publish a complete immutable book addition.
use crate::query_methods::{blob_column, first_column, optional_blob_column, query_rows, try_for_each_row};
use crate::sync::apply::registers::{canonical_body, record_local};
use crate::{Database, DatabaseError};
use book_model::{BookFormat, BookRecord};
use include_sqlite_sql::include_sql;
use library_replica::{EnrichmentResult, LibraryStateEntry};
use metadata_contract::books::{BookEnrichment, BookEnrichmentStatus};
use rusqlite::{Connection, Error as SqlError, ToSql, params_from_iter, types::Type};
use std::collections::HashMap;
use sync_common::BookId;
use sync_common::StateKey;
use sync_common::VersionKey;

pub const FAILURE_RETRY_SECONDS: i64 = 60;

/// What the device stores about a book the metadata server hasn't answered.
#[derive(Clone, Debug)]
pub struct EnrichmentCandidate {
    pub book_id: BookId,
    pub request_key: String,
    /// `description` is empty: the file's own description always wins.
    pub record: BookRecord,
    pub audiobook: bool,
    pub recording: Option<metadata_contract::audible::LookupRequest>,
    /// Local, never synchronized.
    pub filenames: Vec<String>,
}

/// Immutable draft bytes identify the exact lookup input, including recording evidence.
fn facts_input(connection: &Connection, hash: &BookId) -> Result<Option<Vec<u8>>, DatabaseError> {
    Ok(first_column(|read| connection.facts_metadata_body(hash, read))?)
}

fn request_key(body: &[u8]) -> String {
    book_model::agent_id_from_migration_seed(body).to_string()
}

/// The immutable result shared by every device in this library.
pub fn current_answer(connection: &Connection, hash: &BookId) -> Result<Option<EnrichmentResult>, DatabaseError> {
    let Some((bytes, _)) = canonical_body(connection, "book_add", &StateKey::Book(*hash), "")? else { return Ok(None) };
    match sync_common::wire::decode::<LibraryStateEntry>(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? {
        LibraryStateEntry::BookAdd { value, .. } => Ok(value.enrichment),
        _ => Err(DatabaseError::message("invalid stored enrichment result")),
    }
}

impl Database {
    /// Books in `scope` whose current facts have no definitive answer.
    /// Successful answers and definite misses do not expire.
    pub fn enrichment_candidates(&self, scope: &[BookId], now: i64) -> Result<Vec<EnrichmentCandidate>, DatabaseError> {
        let scope = serde_json::to_string(scope).map_err(DatabaseError::operation)?;
        let mut candidates = Vec::new();
        self.connection.facts_candidates(&scope, now, |row| {
            let hash = row.get(0)?;
            let format: Option<i64> = row.get(1)?;
            let body = blob_column(row, 2)?;
            let key = request_key(body);
            let LibraryStateEntry::BookAdd { value, .. } =
                sync_common::wire::decode::<LibraryStateEntry>(body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(|error| SqlError::FromSqlConversionFailure(2, Type::Blob, Box::new(error)))?
            else {
                return Ok(());
            };
            let recording = value.recording;
            let audiobook = matches!(format.and_then(BookFormat::from_code), Some(BookFormat::M4b | BookFormat::Mp3Folder));
            let record = BookRecord { metadata: value.record.metadata, description: String::new() };
            candidates.push(EnrichmentCandidate { book_id: hash, request_key: key, record, audiobook, recording, filenames: Vec::new() });
            Ok(())
        })?;
        if candidates.is_empty() {
            return Ok(candidates);
        }
        let hashes = candidates.iter().map(|candidate| candidate.book_id).collect::<Vec<_>>();
        let requested = serde_json::to_string(&hashes).map_err(DatabaseError::operation)?;
        self.connection.facts_filenames_batch(&requested, |row| {
            let index = usize::try_from(row.get::<_, i64>(0)?).map_err(|error| SqlError::FromSqlConversionFailure(0, Type::Integer, Box::new(error)))?;
            candidates[index].filenames.push(row.get(1)?);
            Ok(())
        })?;
        Ok(candidates)
    }

    /// Stores the server's answer for the facts sent under `request_key`.
    pub fn record_enrichment(&self, hash: &BookId, request_key: &str, result: BookEnrichment, now: i64) -> Result<(), DatabaseError> {
        self.write(|transaction| record_enrichment_on(transaction, hash, request_key, result, now))
    }

    /// Stores one response batch atomically, checking each book's current
    /// answer and request key under the writer. Results are consumed directly;
    /// projection and commit work runs once for the batch.
    pub fn record_enrichments<'a>(&self, hashes: &[BookId], results: impl IntoIterator<Item = (&'a BookId, &'a str, BookEnrichment)>, now: i64) -> Result<(), DatabaseError> {
        let hashes = serde_json::to_string(hashes).map_err(DatabaseError::operation)?;
        self.write(|transaction| {
            let mut inputs = HashMap::new();
            try_for_each_row(
                |read| transaction.facts_inputs_batch(&hashes, read),
                |row| {
                    let hash = row.get::<_, BookId>(0)?;
                    let record = optional_blob_column(row, 2)?;
                    let answered = if let Some(record) = record {
                        sync_common::wire::decode::<VersionKey>(blob_column(row, 3)?, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
                        let LibraryStateEntry::BookAdd { value, .. } = sync_common::wire::decode(record, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? else {
                            return Err(DatabaseError::message("invalid stored enrichment result"));
                        };
                        value.enrichment.is_some()
                    } else {
                        false
                    };
                    let body = if answered { None } else { optional_blob_column(row, 1)?.map(<[u8]>::to_vec) };
                    inputs.insert(hash, (body, record.is_some()));
                    Ok(())
                },
            )?;
            let mut failures = Vec::new();
            let mut additions = Vec::new();
            for (hash, request_key, result) in results {
                let Some((body, record_present)) = inputs.get_mut(hash) else { continue };
                let Some(bytes) = body.as_deref() else { continue };
                if request_key != self::request_key(bytes) {
                    continue;
                }
                if result.status == BookEnrichmentStatus::Failed {
                    failures.push(*hash);
                    continue;
                }
                let LibraryStateEntry::BookAdd { value: mut addition, .. } = sync_common::wire::decode(bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? else {
                    return Err(DatabaseError::message("invalid local book draft"));
                };
                match result.status {
                    BookEnrichmentStatus::Found => {
                        let record = result.record.ok_or_else(|| DatabaseError::message("found enrichment has no record"))?;
                        addition.apply_enrichment(record, EnrichmentResult::Found { toc: result.toc, cover_id: result.cover.map(|cover| cover.id) });
                    }
                    BookEnrichmentStatus::NoMatch => addition.enrichment = Some(EnrichmentResult::NoMatch),
                    BookEnrichmentStatus::Failed => unreachable!(),
                }
                let value = LibraryStateEntry::BookAdd { book_id: *hash, value: addition };
                if *record_present {
                    value.validate().map_err(DatabaseError::operation)?;
                } else {
                    additions.push(value);
                    *body = None;
                }
            }
            if !failures.is_empty() {
                let failures = serde_json::to_string(&failures).map_err(DatabaseError::operation)?;
                transaction.facts_record_failures_batch(&failures, now.saturating_add(FAILURE_RETRY_SECONDS))?;
            }
            crate::sync::apply::records::insert_new_batch(transaction, additions)?;
            Ok(())
        })
    }

    /// Records a catalog book's record as its answer, so the metadata server
    /// is not asked about it. The catalog already enriched and curated it.
    /// Does nothing once the book has an answer.
    pub fn record_catalog_answer(&self, hash: &BookId, record: BookRecord) -> Result<(), DatabaseError> {
        self.write(|transaction| record_catalog_answer(transaction, hash, record))
    }

    /// Answered books in `scope` still eligible for direct cover lookup.
    pub fn enrichment_cover_candidates(&self, scope: &[BookId]) -> Result<Vec<BookId>, DatabaseError> {
        let scope = serde_json::to_string(scope).map_err(DatabaseError::operation)?;
        Ok(query_rows(|read| self.connection.facts_cover_candidates(&scope, read), |row| row.get::<_, BookId>(0))?)
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

    pub fn enrichment_cover_inputs(&self, hashes: &[BookId]) -> Result<Vec<(BookId, Option<BookRecord>, Option<String>, Option<Vec<u8>>)>, DatabaseError> {
        let hashes = serde_json::to_string(hashes).map_err(DatabaseError::operation)?;
        let mut inputs = Vec::new();
        try_for_each_row(
            |read| self.connection.facts_cover_inputs_batch(&hashes, read),
            |row| {
                let record = if let Some(body) = optional_blob_column(row, 1)? {
                    sync_common::wire::decode::<VersionKey>(blob_column(row, 2)?, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)?;
                    let LibraryStateEntry::BookAdd { value, .. } = sync_common::wire::decode(body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? else {
                        return Err(DatabaseError::message("invalid stored book addition"));
                    };
                    let mut record = value.record;
                    record.description.clear();
                    Some(record)
                } else {
                    None
                };
                inputs.push((row.get(0)?, record, row.get(3)?, optional_blob_column(row, 2)?.map(<[u8]>::to_vec)));
                Ok(())
            },
        )?;
        Ok(inputs)
    }

    pub fn mark_enrichment_covers_checked(&self, hashes: &[BookId]) -> Result<(), DatabaseError> {
        if hashes.is_empty() {
            return Ok(());
        }
        let hashes = serde_json::to_string(hashes).map_err(DatabaseError::operation)?;
        self.connection.facts_mark_covers_checked_batch(&hashes)?;
        Ok(())
    }

    /// A later canonical addition must retain its own unchecked cover work.
    pub fn mark_enrichment_covers_checked_if_current(&self, completed: &[(BookId, Option<Vec<u8>>)]) -> Result<(), DatabaseError> {
        if completed.is_empty() {
            return Ok(());
        }
        let mut sql = String::from("WITH requested(content_hash,publication) AS (VALUES ");
        let mut bindings: Vec<&dyn ToSql> = Vec::with_capacity(completed.len() * 2);
        for (index, (hash, publication)) in completed.iter().enumerate() {
            if index != 0 {
                sql.push(',');
            }
            sql.push_str("(?,?)");
            bindings.extend([hash as &dyn ToSql, publication as &dyn ToSql]);
        }
        sql.push_str(
            ") INSERT INTO book_transfer(content_hash,cover_checked)
            SELECT b.content_hash,1 FROM requested JOIN book b ON b.content_hash=requested.content_hash
            LEFT JOIN sync_book_record record ON record.content_hash=b.content_hash AND record.state_kind='book_add'
            WHERE record.publication IS requested.publication
            ON CONFLICT(content_hash) DO UPDATE SET cover_checked=1 WHERE book_transfer.cover_checked=0",
        );
        self.connection.prepare(&sql)?.execute(params_from_iter(bindings))?;
        Ok(())
    }
}

include_sql!("src/enrichment/sql/facts.sql");

fn record_enrichment_on(transaction: &Connection, hash: &BookId, request_key: &str, result: BookEnrichment, now: i64) -> Result<(), DatabaseError> {
    if current_answer(transaction, hash)?.is_some() {
        return Ok(());
    }
    let Some(body) = facts_input(transaction, hash)? else { return Ok(()) };
    if request_key != self::request_key(&body) {
        return Ok(()); // The addition changed while this request was in flight.
    }
    let value = match result.status {
        BookEnrichmentStatus::Failed => {
            transaction.facts_record_failure(hash, now.saturating_add(FAILURE_RETRY_SECONDS))?;
            return Ok(());
        }
        BookEnrichmentStatus::Found => EnrichmentResult::Found { toc: result.toc, cover_id: result.cover.map(|cover| cover.id) },
        BookEnrichmentStatus::NoMatch => EnrichmentResult::NoMatch,
    };
    let LibraryStateEntry::BookAdd { value: mut addition, .. } = sync_common::wire::decode::<LibraryStateEntry>(&body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? else {
        return Err(DatabaseError::message("invalid local book draft"));
    };
    if result.status == BookEnrichmentStatus::Found {
        let record = result.record.ok_or_else(|| DatabaseError::message("found enrichment has no record"))?;
        addition.apply_enrichment(record, value);
    } else {
        addition.enrichment = Some(value);
    }
    record_local(transaction, LibraryStateEntry::BookAdd { book_id: *hash, value: addition })?;
    Ok(())
}

/// Completes an import's catalog answer under the caller's writer transaction.
pub(crate) fn record_catalog_answer(transaction: &rusqlite::Transaction<'_>, hash: &BookId, record: BookRecord) -> Result<(), DatabaseError> {
    if current_answer(transaction, hash)?.is_some() {
        return Ok(());
    }
    let Some(body) = facts_input(transaction, hash)? else { return Ok(()) };
    let LibraryStateEntry::BookAdd { value: mut addition, .. } = sync_common::wire::decode::<LibraryStateEntry>(&body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? else {
        return Err(DatabaseError::message("invalid local book draft"));
    };
    addition.apply_enrichment(record, EnrichmentResult::Found { toc: None, cover_id: None });
    record_local(transaction, LibraryStateEntry::BookAdd { book_id: *hash, value: addition })?;
    Ok(())
}

// Async access owns its arguments until the synchronous database call completes.
database_methods! {
    fn record_enrichment_answers(hashes: Vec<BookId>, answers: Vec<(BookId, String, BookEnrichment)>, now: i64) -> ();
    arguments (Vec<BookId>, Vec<(BookId, String, BookEnrichment)>, i64) = (hashes, answers, now);
    execute database, arguments => database.record_enrichments(&arguments.0, arguments.1.iter().map(|(book, key, result)| (book, key.as_str(), result.clone())), arguments.2);

    fn enrichment_candidates(scope: &[BookId], now: i64) -> Vec<EnrichmentCandidate>;
    arguments (Vec<BookId>, i64,) = (scope.to_vec(), now,);
    execute database, arguments => database.enrichment_candidates(&arguments.0, arguments.1);

    fn record_enrichment(hash: &BookId, request_key: &str, result: BookEnrichment, now: i64) -> ();
    arguments (BookId, String, BookEnrichment, i64,) = (hash.clone(), request_key.to_owned(), result, now,);
    execute database, arguments => database.record_enrichment(&arguments.0, &arguments.1, arguments.2, arguments.3);

    fn record_catalog_answer(hash: &BookId, record: BookRecord) -> ();
    arguments (BookId, BookRecord,) = (hash.clone(), record,);
    execute database, arguments => database.record_catalog_answer(&arguments.0, arguments.1);

    fn enrichment_cover_candidates(scope: &[BookId]) -> Vec<BookId>;
    arguments (Vec<BookId>,) = (scope.to_vec(),);
    execute database, arguments => database.enrichment_cover_candidates(&arguments.0);

    fn enrichment_cover_record(hash: &BookId) -> Option<BookRecord>;
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.enrichment_cover_record(&arguments.0);

    fn enrichment_next_retry() -> Option<i64>;
    arguments () = ();
    execute database, arguments => database.enrichment_next_retry();

    fn mark_enrichment_cover_checked(hash: &BookId) -> ();
    arguments (BookId,) = (hash.clone(),);
    execute database, arguments => database.mark_enrichment_cover_checked(&arguments.0);

    fn enrichment_cover_inputs(hashes: &[BookId]) -> Vec<(BookId, Option<BookRecord>, Option<String>, Option<Vec<u8>>)>;
    arguments (Vec<BookId>,) = (hashes.to_vec(),);
    execute database, arguments => database.enrichment_cover_inputs(&arguments.0);

    fn mark_enrichment_covers_checked(hashes: &[BookId]) -> ();
    arguments (Vec<BookId>,) = (hashes.to_vec(),);
    execute database, arguments => database.mark_enrichment_covers_checked(&arguments.0);

    fn mark_enrichment_covers_checked_if_current(completed: &[(BookId, Option<Vec<u8>>)]) -> ();
    arguments (Vec<(BookId, Option<Vec<u8>>)>,) = (completed.to_vec(),);
    execute database, arguments => database.mark_enrichment_covers_checked_if_current(&arguments.0);
}

impl crate::DatabaseClient {
    pub async fn record_enrichments<'a>(&self, hashes: &[BookId], results: impl IntoIterator<Item = (&'a BookId, &'a str, BookEnrichment)>, now: i64) -> Result<(), DatabaseError> {
        self.record_enrichment_answers(hashes.to_vec(), results.into_iter().map(|(book, key, result)| (*book, key.to_owned(), result)).collect(), now).await
    }
}
