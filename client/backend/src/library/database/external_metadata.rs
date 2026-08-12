use super::*;
use library_model::{PublicationMetadata, PublicationSubject, canonical_isbn};
use rusqlite::OptionalExtension;

#[derive(Clone, Debug)]
pub(crate) struct SubjectEnrichmentCandidate {
    pub content_hash: ContentHash,
    pub title: String,
    pub isbn: String,
}

pub(crate) fn subject_enrichment_candidates(conn: &Connection, provider_id: &str, limit: usize, retry_failed: bool) -> Result<Vec<SubjectEnrichmentCandidate>, rusqlite::Error> {
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let retry_failed = i32::from(retry_failed);
    let mut statement = conn.prepare(
        "SELECT book.content_hash, COALESCE(book.title, ''), MIN(identifier.canonical_value)
         FROM book
         JOIN book_identifier identifier ON identifier.book_row_id=book.row_id
         WHERE book.deleted_at IS NULL
           AND book.hidden_at IS NULL
           AND identifier.scheme='isbn'
           AND identifier.canonical_value IS NOT NULL
           AND NOT EXISTS(SELECT 1 FROM book_subject subject WHERE subject.book_row_id=book.row_id)
           AND NOT EXISTS(
               SELECT 1 FROM external_metadata_attempt attempt
               WHERE attempt.book_row_id=book.row_id
                 AND attempt.provider_id=?1
                 AND (?2=0 OR attempt.status <> 'failed')
           )
         GROUP BY book.row_id
         ORDER BY LOWER(COALESCE(book.title, '')), book.content_hash
         LIMIT ?3",
    )?;
    let rows = statement.query_map(rusqlite::params![provider_id, retry_failed, limit], |row| {
        let raw_hash: String = row.get(0)?;
        Ok(SubjectEnrichmentCandidate { content_hash: ContentHash::new(&raw_hash), title: row.get(1)?, isbn: row.get(2)? })
    })?;
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    for candidate in rows {
        let candidate = candidate?;
        if canonical_isbn(&candidate.isbn).is_some() && seen.insert(candidate.content_hash.clone()) {
            candidates.push(candidate);
        }
    }
    Ok(candidates)
}

pub(crate) fn apply_external_subjects(conn: &mut Connection, content_hash: ContentHash, provider_id: &str, isbn: &str, subjects: Vec<PublicationSubject>, status: &str, detail: Option<&str>) -> Result<usize, rusqlite::Error> {
    let transaction = conn.transaction()?;
    let raw_publication = transaction.query_row("SELECT publication_metadata FROM book WHERE content_hash=?1 AND deleted_at IS NULL", [content_hash.as_str()], |row| row.get::<_, Vec<u8>>(0)).optional()?;
    let Some(raw_publication) = raw_publication else {
        return Ok(0);
    };
    let mut imported = 0;
    if !subjects.is_empty() {
        let has_subjects: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM book_subject WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1))", [content_hash.as_str()], |row| row.get(0))?;
        if !has_subjects {
            let mut publication = decode_publication_metadata(&raw_publication)?;
            imported = subjects.len();
            publication.subjects.extend(subjects);
            super::metadata::update_book_publication_metadata_preserving_ids(&transaction, content_hash.clone(), &publication)?;
        }
    }
    let bounded_detail = detail.map(|value| value.chars().take(1_024).collect::<String>());
    transaction.execute(
        "INSERT INTO external_metadata_attempt(book_row_id,provider_id,identifier,status,detail,attempted_at)
         VALUES((SELECT row_id FROM book WHERE content_hash=?1),?2,?3,?4,?5,CAST(strftime('%s','now') AS INTEGER)*1000)
         ON CONFLICT(book_row_id,provider_id) DO UPDATE SET identifier=excluded.identifier,status=excluded.status,detail=excluded.detail,attempted_at=excluded.attempted_at",
        rusqlite::params![content_hash.as_str(), provider_id, isbn, status, bounded_detail],
    )?;
    transaction.commit()?;
    Ok(imported)
}

fn decode_publication_metadata(raw: &[u8]) -> Result<PublicationMetadata, rusqlite::Error> {
    if raw.is_empty() {
        return Ok(PublicationMetadata::default());
    }
    match sync_common::wire::decode(raw, sync_common::wire::MAX_DECODED_REQUEST_BYTES) {
        Ok(metadata) => Ok(metadata),
        Err(sync_common::wire::WireError::UnsupportedVersion(10)) if raw.starts_with(b"BKHM\x0a") => {
            let mut compatible = raw.to_vec();
            compatible[4] = sync_common::wire::header_bytes()[4];
            let metadata = sync_common::wire::decode::<PublicationMetadata>(&compatible, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(|error| invalid_text_column(0, error.to_string()))?;
            let reencoded = sync_common::wire::encode(&metadata).map_err(|error| invalid_text_column(0, error.to_string()))?;
            if reencoded.get(5..) != raw.get(5..) {
                return Err(invalid_text_column(0, "wire version 10 publication payload is not byte-compatible with version 11"));
            }
            log::info!("upgrading a byte-compatible publication metadata value from wire version 10 to 11");
            Ok(metadata)
        }
        Err(error) => Err(invalid_text_column(0, error.to_string())),
    }
}
