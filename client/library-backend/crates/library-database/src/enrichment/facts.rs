//! File facts sent to the metadata server, and its answers. The registers
//! hold only facts; projection overlays the answer for the current facts.
use crate::shared_sql::SharedSql;
use crate::{Database, DatabaseError};
use include_sqlite_sql::include_sql;
use library_replica::LibraryStateEntry;
use metadata_contract::books::{BookEnrichment, BookEnrichmentStatus, BookFacts};
use sync_common::ContentHash;

// Answers and definite misses are asked again after this.
pub const ANSWER_LIFETIME_SECONDS: i64 = 30 * 24 * 60 * 60;
pub const FAILURE_RETRY_SECONDS: i64 = 60;

#[derive(Clone, Debug)]
pub struct EnrichmentCandidate {
    pub content_hash: ContentHash,
    pub request_key: String,
    pub facts: BookFacts,
}

/// What an answer must match: the metadata register's stored bytes and, for
/// an audiobook, its recording evidence.
pub fn facts_input(connection: &rusqlite::Connection, hash: &str) -> Result<Option<(Vec<u8>, Option<String>)>, DatabaseError> {
    let mut body = None;
    connection.facts_metadata_body(hash, |row| {
        body = Some(row.get::<_, Vec<u8>>(0)?);
        Ok(())
    })?;
    let Some(body) = body else { return Ok(None) };
    let mut recording = None;
    connection.shared_audiobook_recording_evidence(hash, |row| {
        recording = Some(row.get::<_, String>(0)?);
        Ok(())
    })?;
    Ok(Some((body, recording)))
}

pub fn request_key(body: &[u8], recording: Option<&str>) -> String {
    let mut seed = body.to_vec();
    seed.extend_from_slice(recording.unwrap_or_default().as_bytes());
    book_model::agent_id_from_migration_seed(&seed).to_string()
}

/// The server's record for the book's current facts, if it found one.
pub fn current_answer(connection: &rusqlite::Connection, hash: &str) -> Result<Option<BookEnrichment>, DatabaseError> {
    let Some((body, recording)) = facts_input(connection, hash)? else { return Ok(None) };
    let mut answer = None;
    connection.facts_answer(hash, |row| {
        answer = Some((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
        Ok(())
    })?;
    let Some((key, response)) = answer else { return Ok(None) };
    if key != request_key(&body, recording.as_deref()) {
        return Ok(None);
    }
    serde_json::from_str(&response).map(Some).map_err(DatabaseError::operation)
}

impl Database {
    /// Books in `scope` whose current facts the server has not answered, or
    /// whose answer is due for renewal.
    pub fn enrichment_candidates(&self, scope: &[ContentHash], now: i64) -> Result<Vec<EnrichmentCandidate>, DatabaseError> {
        let scope = serde_json::to_string(scope).map_err(DatabaseError::operation)?;
        let mut rows = Vec::new();
        self.connection.facts_candidates(&scope, |row| {
            rows.push((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<i64>>(4)?));
            Ok(())
        })?;
        let mut candidates = Vec::new();
        for (hash, format, stored_key, status, retry_at) in rows {
            let Some((body, recording)) = facts_input(&self.connection, &hash)? else { continue };
            let key = request_key(&body, recording.as_deref());
            let waiting = retry_at.is_some_and(|at| at > now);
            if waiting && (stored_key.as_deref() == Some(key.as_str()) || status.as_deref() == Some("failed")) {
                continue;
            }
            let LibraryStateEntry::Metadata { value, .. } = sync_common::wire::decode::<LibraryStateEntry>(&body, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(DatabaseError::operation)? else {
                continue;
            };
            let mut filenames = Vec::new();
            self.connection.facts_filenames(&hash, |row| {
                filenames.push(row.get(0)?);
                Ok(())
            })?;
            let recording = recording.map(|json| serde_json::from_str(&json)).transpose().map_err(DatabaseError::operation)?;
            let audiobook = matches!(format.and_then(book_model::BookFormat::from_code), Some(book_model::BookFormat::M4b | book_model::BookFormat::Mp3Folder));
            let record = book_model::BookRecord { metadata: value, description: String::new() };
            candidates.push(EnrichmentCandidate { content_hash: ContentHash::new(&hash), request_key: key, facts: BookFacts { key: String::new(), record, audiobook, filenames, recording } });
        }
        Ok(candidates)
    }

    /// Stores the server's answer for the facts sent under `request_key`.
    pub fn record_enrichment(&self, hash: &ContentHash, request_key: &str, result: &BookEnrichment, now: i64) -> Result<(), DatabaseError> {
        self.write(|transaction| {
            match result.status {
                BookEnrichmentStatus::Failed => {
                    transaction.facts_record_failure(hash.as_str(), request_key, now + FAILURE_RETRY_SECONDS)?;
                }
                BookEnrichmentStatus::Found => {
                    let response = serde_json::to_string(result).map_err(DatabaseError::operation)?;
                    transaction.facts_record(hash.as_str(), request_key, "found", Some(response.as_str()), now + ANSWER_LIFETIME_SECONDS)?;
                    transaction.facts_mark_dirty(hash.as_str())?;
                }
                BookEnrichmentStatus::NoMatch => {
                    transaction.facts_record(hash.as_str(), request_key, "no_match", None, now + ANSWER_LIFETIME_SECONDS)?;
                    transaction.facts_mark_dirty(hash.as_str())?;
                }
            }
            Ok(())
        })
    }

    /// Books in `scope` whose answer may point at cover art not yet checked.
    pub fn enrichment_cover_candidates(&self, scope: &[ContentHash]) -> Result<Vec<ContentHash>, DatabaseError> {
        let scope = serde_json::to_string(scope).map_err(DatabaseError::operation)?;
        let mut hashes = Vec::new();
        self.connection.facts_cover_candidates(&scope, |row| {
            hashes.push(ContentHash::new(&row.get::<_, String>(0)?));
            Ok(())
        })?;
        Ok(hashes)
    }

    /// Where the current answer says the book's cover can be fetched.
    pub fn enrichment_cover_source(&self, hash: &ContentHash) -> Result<Option<metadata_contract::books::CoverSource>, DatabaseError> {
        Ok(current_answer(&self.connection, hash.as_str())?.and_then(|answer| answer.cover))
    }

    /// The earliest time a live book's enrichment is due to be asked again.
    pub fn enrichment_next_retry(&self) -> Result<Option<i64>, DatabaseError> {
        Ok(self.connection.facts_next_retry(|row| row.get::<_, Option<i64>>(0))?)
    }

    pub fn mark_enrichment_cover_checked(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.connection.facts_mark_cover_checked(hash.as_str())?;
        Ok(())
    }
}

include_sql!("src/enrichment/sql/facts.sql");
