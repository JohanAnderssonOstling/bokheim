use super::*;
use library_model::{Identifier, PublicationMetadata, PublicationSubject, Scheme, Scope, canonical_isbn};
use metadata_contract::{EditionIdentityMatch, EditionTitleMatch, EntitySource, PersonEntity, PlaceEntity, RichWorkMetadataMatch};
use rusqlite::OptionalExtension;
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};

const ALTERNATIVE_EDITION_PROVENANCE: &str = "openlibrary-alternative-edition-match";
pub(super) const RESTORE_ALTERNATIVE_MATCHES_MIGRATION: &str = "migration_restore_automatic_alternative_edition_matches_v1";

#[derive(Deserialize)]
struct LegacyIdentifierType {
    value: String,
    scheme: Option<String>,
}

pub(super) fn migrate_identifier_sources(conn: &Connection) -> Result<(), rusqlite::Error> {
    let mut statement = conn.prepare("SELECT rowid,type_evidence FROM book_identifier WHERE source='legacy_identifier' AND length(type_evidence)>0")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for (row_id, encoded) in rows {
        let Ok(types) = sync_common::wire::decode::<Vec<LegacyIdentifierType>>(&encoded, sync_common::wire::MAX_DECODED_REQUEST_BYTES) else { continue };
        let Some(source) = types.iter().find(|kind| kind.scheme.as_deref() == Some("bokheim:source")).or_else(|| types.iter().find(|kind| kind.scheme.as_deref() == Some("bokheim:provenance"))).map(|kind| kind.value.as_str()) else {
            continue;
        };
        conn.execute("UPDATE book_identifier SET source=?2 WHERE rowid=?1", rusqlite::params![row_id, source])?;
    }
    Ok(())
}

fn is_automatic_alternative_isbn(identifier: &Identifier) -> bool {
    identifier.scheme() == &Scheme::Isbn && identifier.source() == ALTERNATIVE_EDITION_PROVENANCE
}

/// Reverses the short-lived policy which treated every candidate of an
/// ambiguous edition match as if it identified the local publication. Only a
/// identifier carrying that policy's provenance is removed, while embedded,
/// filename-derived, exact, and manually reviewed ISBNs are retained. This is
/// intentionally idempotent and runs at every open because an older synced
/// metadata value can arrive after the first local migration.
pub(super) fn migrate_automatic_alternative_edition_matches(conn: &Connection) -> Result<usize, rusqlite::Error> {
    let previously_restored = conn.query_row("SELECT value FROM sync_metadata WHERE key=?1", [RESTORE_ALTERNATIVE_MATCHES_MIGRATION], |row| row.get::<_, String>(0)).optional()?.and_then(|value| value.parse::<usize>().ok()).unwrap_or(0);

    let mut statement = conn.prepare("SELECT row_id,content_hash,publication_metadata FROM book")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Vec<u8>>(2)?)))?.collect::<Result<Vec<_>, _>>()?;
    drop(statement);

    let transaction = conn.unchecked_transaction()?;
    let mut restored = 0;
    for (book_row_id, raw_hash, raw_publication) in rows {
        let mut publication = decode_publication_metadata(&raw_publication)?;
        if !publication.identifiers.iter().any(is_automatic_alternative_isbn) {
            continue;
        }

        // Positions let us remove only metadata created by the ISBN-dependent
        // enrichers before rewriting the canonical value.
        let mut subject_statement = transaction.prepare(
            "SELECT position FROM book_subject
             WHERE book_row_id=?1 AND (source LIKE 'openlibrary:metadata:%' OR source='library_of_congress')",
        )?;
        let external_subject_positions = subject_statement
            .query_map([book_row_id], |row| {
                let position = row.get::<_, i64>(0)?;
                usize::try_from(position).map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Integer, Box::new(error)))
            })?
            .collect::<Result<BTreeSet<_>, _>>()?;
        drop(subject_statement);

        publication.identifiers.retain(|identifier| !is_automatic_alternative_isbn(identifier));
        publication.subjects = publication.subjects.into_iter().enumerate().filter_map(|(position, subject)| (!external_subject_positions.contains(&position)).then_some(subject)).collect();
        super::metadata::update_book_publication_metadata_preserving_ids(&transaction, ContentHash::new(&raw_hash), &publication)?;

        transaction.execute("DELETE FROM book_external_entity WHERE book_row_id=?1", [book_row_id])?;
        transaction.execute(
            "DELETE FROM external_metadata_attempt
             WHERE book_row_id=?1 AND (
                 provider_id LIKE 'openlibrary_edition_identity_%'
                 OR provider_id LIKE 'openlibrary_rich_%'
                 OR provider_id LIKE 'openlibrary_authors_%'
                 OR provider_id='librarything_thingisbn_v1'
                 OR provider_id='library_of_congress'
             )",
            [book_row_id],
        )?;
        restored += 1;
    }
    transaction.execute(
        "DELETE FROM external_entity
         WHERE (wikidata_id LIKE 'OLP-%' OR wikidata_id LIKE 'OLR-%')
           AND NOT EXISTS(SELECT 1 FROM book_external_entity reference WHERE reference.wikidata_id=external_entity.wikidata_id)",
        [],
    )?;
    let total_restored = previously_restored.saturating_add(restored);
    transaction.execute(
        "INSERT INTO sync_metadata(key,value) VALUES(?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        rusqlite::params![RESTORE_ALTERNATIVE_MATCHES_MIGRATION, total_restored.to_string()],
    )?;
    transaction.commit()?;
    Ok(restored)
}

/// Candidate rows created before classifications were included in manual
/// review. They are persistent local data, so adding a trailing bincode field
/// must not make an existing review page unreadable.
#[derive(Deserialize)]
struct LegacyEditionIdentityMatch {
    canonical_isbn13: String,
    open_library_edition_id: String,
    open_library_work_id: Option<String>,
    title: String,
    work_title: Option<String>,
    title_match: EditionTitleMatch,
    authors: Vec<String>,
    publishers: Vec<String>,
    publication_year: Option<i32>,
    matched_publisher: bool,
    matched_publication_year: bool,
}

pub(super) fn migrate_legacy_edition_identity_candidates(conn: &Connection) -> Result<(), rusqlite::Error> {
    let mut rows = conn.prepare("SELECT rowid,candidate FROM external_metadata_candidate")?;
    let stored = rows.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    drop(rows);

    let transaction = conn.unchecked_transaction()?;
    for (row_id, encoded) in stored {
        if sync_common::wire::decode::<EditionIdentityMatch>(&encoded, metadata_contract::MAX_RESPONSE_BYTES).is_ok() {
            continue;
        }
        let legacy: LegacyEditionIdentityMatch = sync_common::wire::decode(&encoded, metadata_contract::MAX_RESPONSE_BYTES).map_err(|error| invalid_text_column(1, error.to_string()))?;
        let candidate = EditionIdentityMatch {
            canonical_isbn13: legacy.canonical_isbn13,
            open_library_edition_id: legacy.open_library_edition_id,
            open_library_work_id: legacy.open_library_work_id,
            title: legacy.title,
            work_title: legacy.work_title,
            title_match: legacy.title_match,
            authors: legacy.authors,
            publishers: legacy.publishers,
            publication_year: legacy.publication_year,
            matched_publisher: legacy.matched_publisher,
            matched_publication_year: legacy.matched_publication_year,
            classifications: Vec::new(),
        };
        let current = sync_common::wire::encode(&candidate).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        transaction.execute("UPDATE external_metadata_candidate SET candidate=?2 WHERE rowid=?1", rusqlite::params![row_id, current])?;
    }
    transaction.commit()
}

fn unresolved_entity_id(kind: &str, name: &str) -> String {
    let prefix = if kind == "place" { "OLP-" } else { "OLR-" };
    let mut id = String::with_capacity(prefix.len() + name.len() * 2);
    id.push_str(prefix);
    for byte in name.trim().as_bytes() {
        use std::fmt::Write as _;
        let _ = write!(id, "{byte:02x}");
    }
    id
}

#[derive(Clone, Debug)]
pub struct SubjectEnrichmentCandidate {
    pub content_hash: ContentHash,
    pub title: String,
    pub isbn: String,
}

#[derive(Clone, Debug)]
pub struct RichEnrichmentCandidate {
    pub content_hash: ContentHash,
    pub isbns: Vec<String>,
    /// ISBNs which identify the local publication itself. ISBNs attached from
    /// every ambiguous Open Library edition remain useful for locating work
    /// records, but their edition metadata must not be treated as exact
    /// evidence for the local file.
    pub exact_isbns: BTreeSet<String>,
    pub classification_count: usize,
}

#[derive(Clone, Debug)]
pub struct AuthorEnrichmentCredit {
    pub position: usize,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct AuthorEnrichmentCandidate {
    pub content_hash: ContentHash,
    pub isbn: String,
    pub authors: Vec<AuthorEnrichmentCredit>,
}

#[derive(Clone, Debug)]
pub struct EditionIdentityCandidate {
    pub content_hash: ContentHash,
    pub title: String,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub publication_year: Option<i32>,
}

pub struct EditionIdentityReviewRow {
    pub content_hash: ContentHash,
    pub title: String,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub publication_year: Option<i32>,
    pub status: String,
    pub detail: Option<String>,
    pub candidates: Vec<EditionIdentityMatch>,
    pub candidates_truncated: bool,
}

pub fn record_external_metadata_failures(conn: &mut Connection, provider_id: &str, candidates: &[(ContentHash, String)], detail: &str) -> Result<(), rusqlite::Error> {
    let transaction = conn.transaction()?;
    let bounded_detail = detail.chars().take(1_024).collect::<String>();
    for (content_hash, identifier) in candidates {
        transaction.execute(
            "INSERT INTO external_metadata_attempt(book_row_id,provider_id,identifier,status,detail,attempted_at)
             VALUES((SELECT row_id FROM book WHERE content_hash=?1),?2,?3,'failed',?4,CAST(strftime('%s','now') AS INTEGER)*1000)
             ON CONFLICT(book_row_id,provider_id) DO UPDATE SET identifier=excluded.identifier,status='failed',detail=excluded.detail,attempted_at=excluded.attempted_at",
            rusqlite::params![content_hash.as_str(), provider_id, identifier, bounded_detail],
        )?;
    }
    transaction.commit()
}

pub fn record_external_metadata_attempt(conn: &Connection, content_hash: &ContentHash, provider_id: &str, identifier: &str, status: &str, detail: Option<&str>) -> Result<(), rusqlite::Error> {
    let bounded_detail = detail.map(|value| value.chars().take(1_024).collect::<String>());
    conn.execute(
        "INSERT INTO external_metadata_attempt(book_row_id,provider_id,identifier,status,detail,attempted_at)
         VALUES((SELECT row_id FROM book WHERE content_hash=?1),?2,?3,?4,?5,CAST(strftime('%s','now') AS INTEGER)*1000)
         ON CONFLICT(book_row_id,provider_id) DO UPDATE SET identifier=excluded.identifier,status=excluded.status,detail=excluded.detail,attempted_at=excluded.attempted_at",
        rusqlite::params![content_hash.as_str(), provider_id, identifier, status, bounded_detail],
    )?;
    Ok(())
}

pub fn edition_identity_reviews(conn: &Connection, provider_id: &str) -> Result<Vec<EditionIdentityReviewRow>, rusqlite::Error> {
    let mut reviews = conn
        .prepare(
            "SELECT book.row_id,book.content_hash,COALESCE(book.title,''),book.publication_metadata,attempt.status,attempt.detail
             FROM external_metadata_attempt attempt
             JOIN book ON book.row_id=attempt.book_row_id
             WHERE attempt.provider_id=?1
               AND attempt.status IN ('ambiguous','no_match','failed')
               AND book.deleted_at IS NULL AND book.hidden_at IS NULL
               AND EXISTS(SELECT 1 FROM book_dir placement WHERE placement.book_row_id=book.row_id AND placement.deleted_at IS NULL)
               AND NOT EXISTS(SELECT 1 FROM book_subject subject WHERE subject.book_row_id=book.row_id)
               AND NOT EXISTS(SELECT 1 FROM book_external_entity entity WHERE entity.book_row_id=book.row_id)
             ORDER BY CASE attempt.status WHEN 'ambiguous' THEN 0 WHEN 'no_match' THEN 1 ELSE 2 END,
                      LOWER(COALESCE(book.title,'')),book.content_hash",
        )?
        .query_map([provider_id], |row| Ok((row.get::<_, i64>(0)?, ContentHash::new(&row.get::<_, String>(1)?), row.get::<_, String>(2)?, row.get::<_, Vec<u8>>(3)?, row.get::<_, String>(4)?, row.get::<_, Option<String>>(5)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let review_ids = reviews.iter().map(|review| review.0).collect::<BTreeSet<_>>();
    let mut authors = HashMap::<i64, Vec<String>>::new();
    let mut author_statement = conn.prepare(
        "SELECT contributor.book_row_id,contributor.name
         FROM external_metadata_attempt attempt
         JOIN book_contributor contributor ON contributor.book_row_id=attempt.book_row_id AND contributor.role=X'617574'
         WHERE attempt.provider_id=?1 AND attempt.status IN ('ambiguous','no_match','failed')
         ORDER BY contributor.book_row_id,contributor.position",
    )?;
    let author_rows = author_statement.query_map([provider_id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
    for row in author_rows {
        let (book_row_id, name) = row?;
        if review_ids.contains(&book_row_id) {
            authors.entry(book_row_id).or_default().push(name);
        }
    }
    let mut candidates = HashMap::<i64, Vec<EditionIdentityMatch>>::new();
    let mut candidate_statement = conn.prepare(
        "SELECT candidate.book_row_id,candidate.candidate
         FROM external_metadata_attempt attempt
         JOIN external_metadata_candidate candidate
           ON candidate.book_row_id=attempt.book_row_id AND candidate.provider_id=attempt.provider_id
         WHERE attempt.provider_id=?1 AND attempt.status IN ('ambiguous','no_match','failed')
         ORDER BY candidate.book_row_id,candidate.position",
    )?;
    let candidate_rows = candidate_statement.query_map([provider_id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?)))?;
    for row in candidate_rows {
        let (book_row_id, encoded) = row?;
        if review_ids.contains(&book_row_id) {
            let candidate = sync_common::wire::decode(&encoded, metadata_contract::MAX_RESPONSE_BYTES).map_err(|error| invalid_text_column(1, error.to_string()))?;
            candidates.entry(book_row_id).or_default().push(candidate);
        }
    }
    let mut result = Vec::with_capacity(reviews.len());
    for (book_row_id, content_hash, title, raw_publication, status, detail) in reviews.drain(..) {
        let publication = decode_publication_metadata(&raw_publication)?;
        let publishers = publication.publishers.iter().map(|publisher| publisher.name().as_str().to_owned()).collect::<Vec<_>>();
        let publication_year = library_model::unique_publication_year(publication.dates.iter().map(|date| date.value()));
        let candidates_truncated = detail.as_deref() == Some("candidate list was truncated");
        result.push(EditionIdentityReviewRow {
            content_hash,
            title,
            authors: authors.remove(&book_row_id).unwrap_or_default(),
            publishers,
            publication_year,
            status,
            detail,
            candidates: candidates.remove(&book_row_id).unwrap_or_default(),
            candidates_truncated,
        });
    }
    Ok(result)
}

pub fn update_edition_identity_candidate_classifications(conn: &mut Connection, provider_id: &str, candidates: &[(ContentHash, EditionIdentityMatch)]) -> Result<usize, rusqlite::Error> {
    let transaction = conn.transaction()?;
    let mut updated = 0;
    for (content_hash, candidate) in candidates {
        let encoded = sync_common::wire::encode(candidate).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        updated += transaction.execute(
            "UPDATE external_metadata_candidate SET candidate=?5
             WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)
               AND provider_id=?2 AND edition_id=?3 AND isbn13=?4",
            rusqlite::params![content_hash.as_str(), provider_id, candidate.open_library_edition_id, candidate.canonical_isbn13, encoded,],
        )?;
    }
    transaction.commit()?;
    Ok(updated)
}

pub fn apply_manual_edition_identity(conn: &mut Connection, content_hash: ContentHash, provider_id: &str, isbn: &str) -> Result<bool, rusqlite::Error> {
    let canonical = canonical_isbn(isbn).ok_or_else(|| invalid_text_column(0, "manual ISBN is invalid"))?;
    apply_edition_identity_with_provenance(conn, content_hash, provider_id, "manual-review", &[canonical.as_str()], "updated", Some("ISBN entered during manual review"), &[], "manual-isbn-review")
}

pub fn apply_filename_isbn(conn: &mut Connection, content_hash: ContentHash, provider_id: &str, isbn: &str) -> Result<bool, rusqlite::Error> {
    let canonical = canonical_isbn(isbn).ok_or_else(|| invalid_text_column(0, "filename ISBN is invalid"))?;
    apply_edition_identity_with_provenance(conn, content_hash, provider_id, &canonical, &[canonical.as_str()], "updated", Some("checksum-valid ISBN extracted from filename"), &[], "filename-isbn")
}

pub fn reject_edition_identity_candidates(conn: &mut Connection, content_hash: ContentHash, provider_id: &str) -> Result<(), rusqlite::Error> {
    apply_edition_identity(conn, content_hash, provider_id, "manual-review", None, "no_match", Some("No suitable edition selected during manual review"), &[]).map(|_| ())
}

pub fn apply_reviewed_edition_identity(conn: &mut Connection, content_hash: ContentHash, provider_id: &str, edition_id: &str, isbn13: &str) -> Result<bool, rusqlite::Error> {
    let encoded = conn
        .query_row(
            "SELECT candidate FROM external_metadata_candidate
             WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND provider_id=?2 AND edition_id=?3 AND isbn13=?4",
            rusqlite::params![content_hash.as_str(), provider_id, edition_id, isbn13],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()?;
    let Some(encoded) = encoded else { return Ok(false) };
    let candidate: EditionIdentityMatch = sync_common::wire::decode(&encoded, metadata_contract::MAX_RESPONSE_BYTES).map_err(|error| invalid_text_column(0, error.to_string()))?;
    apply_edition_identity(conn, content_hash, provider_id, &candidate.open_library_edition_id, Some(&candidate.canonical_isbn13), "updated", Some("selected during manual review"), &[])
}

pub fn edition_identity_candidates(conn: &Connection, provider_id: &str, limit: usize, retry_failed: bool) -> Result<Vec<EditionIdentityCandidate>, rusqlite::Error> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut statement = conn.prepare(
        "SELECT book.content_hash,COALESCE(book.title,''),book.publication_metadata
         FROM book
         WHERE book.deleted_at IS NULL AND book.hidden_at IS NULL
           AND NOT EXISTS(SELECT 1 FROM book_identifier identifier WHERE identifier.book_row_id=book.row_id AND identifier.scheme='isbn' AND identifier.canonical_value IS NOT NULL)
           AND NOT EXISTS(
               SELECT 1 FROM external_metadata_attempt attempt
               WHERE attempt.book_row_id=book.row_id AND attempt.provider_id=?1
                 AND attempt.attempted_at >= CAST(strftime('%s','now') AS INTEGER)*1000-2592000000
                 AND (?2=0 OR attempt.status <> 'failed'))
         ORDER BY book.content_hash",
    )?;
    let rows = statement.query_map(rusqlite::params![provider_id, i32::from(retry_failed)], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Vec<u8>>(2)?)))?;
    let mut candidates = Vec::new();
    for row in rows {
        let (raw_hash, title, raw_publication) = row?;
        if title.trim().is_empty() {
            continue;
        }
        let publication = decode_publication_metadata(&raw_publication)?;
        let publishers = publication.publishers.iter().map(|publisher| publisher.name().as_str().to_owned()).collect::<Vec<_>>();
        let publication_year = library_model::unique_publication_year(publication.dates.iter().map(|date| date.value()));
        let content_hash = ContentHash::new(&raw_hash);
        let mut authors = conn.prepare_cached("SELECT name FROM book_contributor WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND role=X'617574' ORDER BY position")?;
        let authors = authors.query_map([content_hash.as_str()], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        candidates.push(EditionIdentityCandidate { content_hash, title, authors, publishers, publication_year });
        if candidates.len() >= limit {
            break;
        }
    }
    Ok(candidates)
}

pub fn edition_identity_candidate(conn: &Connection, provider_id: &str, content_hash: &ContentHash) -> Result<Option<EditionIdentityCandidate>, rusqlite::Error> {
    let row = conn
        .query_row(
            "SELECT COALESCE(book.title,''),book.publication_metadata
             FROM book
             WHERE book.content_hash=?1 AND book.deleted_at IS NULL AND book.hidden_at IS NULL
               AND NOT EXISTS(SELECT 1 FROM book_identifier identifier WHERE identifier.book_row_id=book.row_id AND identifier.scheme='isbn' AND identifier.canonical_value IS NOT NULL)",
            [content_hash.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )
        .optional()?;
    let Some((title, raw_publication)) = row else { return Ok(None) };
    if title.trim().is_empty() {
        return Ok(None);
    }
    let has_attempt: bool =
        conn.query_row("SELECT EXISTS(SELECT 1 FROM external_metadata_attempt WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND provider_id=?2)", rusqlite::params![content_hash.as_str(), provider_id], |row| row.get(0))?;
    if !has_attempt {
        return Ok(None);
    }
    let publication = decode_publication_metadata(&raw_publication)?;
    let publishers = publication.publishers.iter().map(|publisher| publisher.name().as_str().to_owned()).collect::<Vec<_>>();
    let publication_year = library_model::unique_publication_year(publication.dates.iter().map(|date| date.value()));
    let mut statement = conn.prepare_cached(
        "SELECT name FROM book_contributor
         WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND role=X'617574'
         ORDER BY position",
    )?;
    let authors = statement.query_map([content_hash.as_str()], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
    Ok(Some(EditionIdentityCandidate { content_hash: content_hash.clone(), title, authors, publishers, publication_year }))
}

pub fn apply_edition_identity(
    conn: &mut Connection, content_hash: ContentHash, provider_id: &str, identifier: &str, isbn: Option<&str>, status: &str, detail: Option<&str>, candidates: &[EditionIdentityMatch],
) -> Result<bool, rusqlite::Error> {
    apply_edition_identity_with_provenance(conn, content_hash, provider_id, identifier, &isbn.into_iter().collect::<Vec<_>>(), status, detail, candidates, "openlibrary-edition-match")
}

pub fn apply_alternative_edition_isbns(conn: &mut Connection, content_hash: ContentHash, provider_id: &str, identifier: &str, isbns: &[&str], detail: Option<&str>) -> Result<bool, rusqlite::Error> {
    apply_edition_identity_with_provenance(conn, content_hash, provider_id, identifier, isbns, "updated", detail, &[], "openlibrary-alternative-edition-match")
}

fn apply_edition_identity_with_provenance(
    conn: &mut Connection, content_hash: ContentHash, provider_id: &str, identifier: &str, isbns: &[&str], status: &str, detail: Option<&str>, candidates: &[EditionIdentityMatch], provenance_value: &str,
) -> Result<bool, rusqlite::Error> {
    let transaction = conn.transaction()?;
    let raw_publication = transaction.query_row("SELECT publication_metadata FROM book WHERE content_hash=?1 AND deleted_at IS NULL", [content_hash.as_str()], |row| row.get::<_, Vec<u8>>(0)).optional()?;
    let Some(raw_publication) = raw_publication else { return Ok(false) };
    let mut updated = false;
    if !isbns.is_empty() {
        let already_has_isbn: bool =
            transaction
                .query_row("SELECT EXISTS(SELECT 1 FROM book_identifier WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND scheme='isbn' AND canonical_value IS NOT NULL)", [content_hash.as_str()], |row| row.get(0))?;
        if !already_has_isbn {
            let mut publication = decode_publication_metadata(&raw_publication)?;
            for isbn in isbns.iter().copied().collect::<BTreeSet<_>>() {
                let inferred = Identifier::new(isbn, Scheme::Isbn, Scope::Edition, provenance_value).map_err(|error| invalid_text_column(0, error.to_string()))?;
                publication.identifiers.push(inferred);
            }
            super::metadata::update_book_publication_metadata_preserving_ids(&transaction, content_hash.clone(), &publication)?;
            updated = true;
        }
    }
    let bounded_detail = detail.map(|value| value.chars().take(1_024).collect::<String>());
    transaction.execute(
        "INSERT INTO external_metadata_attempt(book_row_id,provider_id,identifier,status,detail,attempted_at)
         VALUES((SELECT row_id FROM book WHERE content_hash=?1),?2,?3,?4,?5,CAST(strftime('%s','now') AS INTEGER)*1000)
         ON CONFLICT(book_row_id,provider_id) DO UPDATE SET identifier=excluded.identifier,status=excluded.status,detail=excluded.detail,attempted_at=excluded.attempted_at",
        rusqlite::params![content_hash.as_str(), provider_id, identifier, status, bounded_detail],
    )?;
    transaction.execute("DELETE FROM external_metadata_candidate WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND provider_id=?2", rusqlite::params![content_hash.as_str(), provider_id])?;
    for (position, candidate) in candidates.iter().enumerate() {
        let encoded = sync_common::wire::encode(candidate).map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        transaction.execute(
            "INSERT INTO external_metadata_candidate(book_row_id,provider_id,position,isbn13,edition_id,candidate)
             VALUES((SELECT row_id FROM book WHERE content_hash=?1),?2,?3,?4,?5,?6)",
            rusqlite::params![content_hash.as_str(), provider_id, i64::try_from(position).unwrap_or(i64::MAX), candidate.canonical_isbn13, candidate.open_library_edition_id, encoded],
        )?;
    }
    transaction.commit()?;
    Ok(updated)
}

pub fn rich_enrichment_candidates(conn: &Connection, provider_id: &str, limit: usize, retry_failed: bool) -> Result<Vec<RichEnrichmentCandidate>, rusqlite::Error> {
    let mut statement = conn.prepare(
        "WITH candidates AS (
             SELECT book.row_id,book.content_hash,(
                 SELECT COUNT(*) FROM book_subject subject
                 WHERE subject.book_row_id=book.row_id
                   AND LOWER(COALESCE(subject.authority,'')) IN ('lcc','ddc')
             ) AS classification_count
             FROM book
             WHERE book.deleted_at IS NULL
               AND book.hidden_at IS NULL
               AND EXISTS(
                   SELECT 1 FROM book_identifier identifier
                   WHERE identifier.book_row_id=book.row_id
                     AND identifier.scheme='isbn'
                     AND identifier.canonical_value IS NOT NULL
               )
               AND NOT EXISTS(
                   SELECT 1 FROM external_metadata_attempt attempt
                   WHERE attempt.book_row_id=book.row_id
                     AND attempt.provider_id=?1
                     AND attempt.attempted_at >= CAST(strftime('%s','now') AS INTEGER)*1000-2592000000
                     AND (?2=0 OR attempt.status <> 'failed')
               )
             ORDER BY classification_count DESC,book.content_hash
             LIMIT ?3
         )
         SELECT candidates.content_hash,identifier.canonical_value,identifier.source,candidates.classification_count
         FROM candidates
         JOIN book_identifier identifier ON identifier.book_row_id=candidates.row_id
         WHERE identifier.scheme='isbn' AND identifier.canonical_value IS NOT NULL
         ORDER BY candidates.classification_count DESC,candidates.content_hash,identifier.canonical_value",
    )?;
    let rows = statement.query_map(rusqlite::params![provider_id, i32::from(retry_failed), i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
        let classification_count = row.get::<_, i64>(3)?;
        let classification_count = usize::try_from(classification_count).map_err(|error| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Integer, Box::new(error)))?;
        Ok((ContentHash::new(&row.get::<_, String>(0)?), row.get::<_, String>(1)?, row.get::<_, String>(2)?, classification_count))
    })?;
    let mut candidates = Vec::<RichEnrichmentCandidate>::new();
    for row in rows {
        let (content_hash, raw_isbn, source, classification_count) = row?;
        let Some(isbn) = canonical_isbn(&raw_isbn) else { continue };
        if candidates.last().is_none_or(|candidate| candidate.content_hash != content_hash) {
            candidates.push(RichEnrichmentCandidate { content_hash, isbns: Vec::new(), exact_isbns: BTreeSet::new(), classification_count });
        }
        let candidate = candidates.last_mut().expect("candidate was inserted above");
        let is_alternative_edition = source == ALTERNATIVE_EDITION_PROVENANCE;
        if !is_alternative_edition {
            candidate.exact_isbns.insert(isbn.clone());
        }
        if candidate.isbns.last() != Some(&isbn) {
            candidate.isbns.push(isbn);
        }
    }
    candidates.retain(|candidate| !candidate.isbns.is_empty());
    Ok(candidates)
}

pub fn apply_rich_enrichment(
    conn: &mut Connection, content_hash: ContentHash, provider_id: &str, isbn: &str, matched: Option<&RichWorkMetadataMatch>, places: &[PlaceEntity], people: &[PersonEntity], subjects: Vec<PublicationSubject>, status: &str,
    detail: Option<&str>, replace_librarything_fallback: bool,
) -> Result<usize, rusqlite::Error> {
    let transaction = conn.transaction()?;
    let raw_publication = transaction.query_row("SELECT publication_metadata FROM book WHERE content_hash=?1 AND deleted_at IS NULL", [content_hash.as_str()], |row| row.get::<_, Vec<u8>>(0)).optional()?;
    let Some(raw_publication) = raw_publication else { return Ok(0) };

    for place in places {
        transaction.execute(
            "INSERT INTO external_entity(wikidata_id,kind,label,latitude_e7,longitude_e7)
             VALUES(?1,'place',?2,?3,?4)
             ON CONFLICT(wikidata_id) DO UPDATE SET kind='place',label=excluded.label,latitude_e7=excluded.latitude_e7,longitude_e7=excluded.longitude_e7",
            rusqlite::params![place.wikidata_id, place.label, place.latitude_e7, place.longitude_e7],
        )?;
    }
    for person in people {
        transaction.execute(
            "INSERT INTO external_entity(wikidata_id,kind,label,birth_year,death_year,fictional)
             VALUES(?1,'person',?2,?3,?4,?5)
             ON CONFLICT(wikidata_id) DO UPDATE SET kind='person',label=excluded.label,birth_year=excluded.birth_year,death_year=excluded.death_year,fictional=excluded.fictional",
            rusqlite::params![person.wikidata_id, person.label, person.birth_year, person.death_year, i32::from(person.fictional)],
        )?;
    }
    for (wikidata_id, aliases) in places.iter().map(|entity| (&entity.wikidata_id, &entity.aliases)).chain(people.iter().map(|entity| (&entity.wikidata_id, &entity.aliases))) {
        transaction.execute("DELETE FROM external_entity_alias WHERE wikidata_id=?1", [wikidata_id])?;
        for alias in aliases {
            transaction.execute("INSERT OR IGNORE INTO external_entity_alias(wikidata_id,alias) VALUES(?1,?2)", rusqlite::params![wikidata_id, alias])?;
        }
    }
    for place in places {
        transaction.execute("DELETE FROM external_place_edge WHERE child_id=?1", [&place.wikidata_id])?;
        for (relation, parent_ids) in [("parent", &place.parent_ids), ("country", &place.country_ids)] {
            for parent_id in parent_ids {
                transaction.execute("INSERT OR IGNORE INTO external_place_edge(child_id,parent_id,relation) VALUES(?1,?2,?3)", rusqlite::params![place.wikidata_id, parent_id, relation])?;
            }
        }
    }

    transaction.execute("DELETE FROM book_external_entity WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)", [content_hash.as_str()])?;
    if let Some(matched) = matched {
        for (kind, references) in [("place", &matched.places), ("person", &matched.people)] {
            for reference in references {
                let entity_id = reference.wikidata_id.clone().unwrap_or_else(|| unresolved_entity_id(kind, &reference.name));
                if reference.wikidata_id.is_none() {
                    transaction.execute(
                        "INSERT INTO external_entity(wikidata_id,kind,label) VALUES(?1,?2,?3)
                         ON CONFLICT(wikidata_id) DO UPDATE SET kind=excluded.kind,label=excluded.label",
                        rusqlite::params![entity_id, kind, reference.name],
                    )?;
                }
                let source = match reference.source {
                    EntitySource::ExactEdition => "exact_edition",
                    EntitySource::Work => "work",
                };
                transaction.execute(
                    "INSERT OR IGNORE INTO book_external_entity(book_row_id,wikidata_id,kind,evidence_name,source)
                     VALUES((SELECT row_id FROM book WHERE content_hash=?1),?2,?3,?4,?5)",
                    rusqlite::params![content_hash.as_str(), entity_id, kind, reference.name, source],
                )?;
            }
        }
    }
    transaction.execute(
        "DELETE FROM external_entity WHERE (wikidata_id LIKE 'OLP-%' OR wikidata_id LIKE 'OLR-%')
         AND NOT EXISTS(SELECT 1 FROM book_external_entity reference WHERE reference.wikidata_id=external_entity.wikidata_id)",
        [],
    )?;

    let mut imported = 0;
    if !subjects.is_empty() || replace_librarything_fallback {
        let mut publication = decode_publication_metadata(&raw_publication)?;
        let before = publication.subjects.len();
        if replace_librarything_fallback {
            let fallback_codes = {
                let mut statement = transaction.prepare(
                    "SELECT LOWER(COALESCE(authority,'')),COALESCE(code,name)
                     FROM book_subject
                     WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)
                       AND LOWER(COALESCE(authority,'')) IN ('lcc','ddc')
                       AND source IN ('dc:subject','openlibrary:metadata:sibling-edition','openlibrary:metadata:work')",
                )?;
                let codes = statement.query_map([content_hash.as_str()], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?.collect::<Result<BTreeSet<_>, _>>()?;
                codes
            };
            publication.subjects.retain(|subject| {
                let authority = subject.authority().unwrap_or_default().to_ascii_lowercase();
                let notation = subject.code().unwrap_or_else(|| subject.name());
                !fallback_codes.contains(&(authority, notation.to_owned()))
            });
        }
        let removed = publication.subjects.len() != before;
        for subject in subjects {
            if !publication.subjects.contains(&subject) {
                publication.subjects.push(subject);
                imported += 1;
            }
        }
        if imported > 0 || removed {
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

pub fn author_enrichment_candidates(conn: &Connection, provider_id: &str, limit: usize, retry_failed: bool) -> Result<Vec<AuthorEnrichmentCandidate>, rusqlite::Error> {
    let mut statement = conn.prepare(
        "SELECT book.content_hash, MIN(identifier.canonical_value)
         FROM book
         JOIN book_identifier identifier ON identifier.book_row_id=book.row_id
         WHERE book.deleted_at IS NULL
           AND book.hidden_at IS NULL
           AND identifier.scheme='isbn'
           AND identifier.canonical_value IS NOT NULL
           AND EXISTS(SELECT 1 FROM book_contributor contributor WHERE contributor.book_row_id=book.row_id AND contributor.role=X'617574')
           AND NOT EXISTS(
               SELECT 1 FROM external_metadata_attempt attempt
               WHERE attempt.book_row_id=book.row_id
                 AND attempt.provider_id=?1
                 AND attempt.attempted_at >= CAST(strftime('%s','now') AS INTEGER)*1000-2592000000
                 AND (?2=0 OR attempt.status <> 'failed')
           )
         GROUP BY book.row_id
         ORDER BY book.content_hash
         LIMIT ?3",
    )?;
    let rows = statement.query_map(rusqlite::params![provider_id, i32::from(retry_failed), i64::try_from(limit).unwrap_or(i64::MAX)], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut candidates = Vec::new();
    for row in rows {
        let (raw_hash, isbn) = row?;
        if canonical_isbn(&isbn).is_none() {
            continue;
        }
        let content_hash = ContentHash::new(&raw_hash);
        let mut authors = conn.prepare_cached(
            "SELECT position,name FROM book_contributor
             WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND role=X'617574'
             ORDER BY position",
        )?;
        let authors = authors
            .query_map([content_hash.as_str()], |row| {
                let position = usize::try_from(row.get::<_, i64>(0)?).map_err(|_| invalid_integer_column(0, "author position must not be negative"))?;
                Ok(AuthorEnrichmentCredit { position, name: row.get(1)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if !authors.is_empty() {
            candidates.push(AuthorEnrichmentCandidate { content_hash, isbn, authors });
        }
    }
    Ok(candidates)
}

pub fn apply_author_enrichment(conn: &mut Connection, content_hash: ContentHash, provider_id: &str, isbn: &str, resolutions: &[(usize, String, ExternalAuthorId)], status: &str, detail: Option<&str>) -> Result<usize, rusqlite::Error> {
    let transaction = conn.transaction()?;
    let mut updated = 0_usize;
    for (position, preferred_name, external_id) in resolutions {
        super::metadata::resolve_book_author_identity(&transaction, content_hash.clone(), *position, preferred_name, external_id)?;
        updated += 1;
    }
    let bounded_detail = detail.map(|value| value.chars().take(1_024).collect::<String>());
    transaction.execute(
        "INSERT INTO external_metadata_attempt(book_row_id,provider_id,identifier,status,detail,attempted_at)
         VALUES((SELECT row_id FROM book WHERE content_hash=?1),?2,?3,?4,?5,CAST(strftime('%s','now') AS INTEGER)*1000)
         ON CONFLICT(book_row_id,provider_id) DO UPDATE SET identifier=excluded.identifier,status=excluded.status,detail=excluded.detail,attempted_at=excluded.attempted_at",
        rusqlite::params![content_hash.as_str(), provider_id, isbn, status, bounded_detail],
    )?;
    transaction.commit()?;
    Ok(updated)
}

pub fn subject_enrichment_candidates(conn: &Connection, provider_id: &str, limit: usize, retry_failed: bool) -> Result<Vec<SubjectEnrichmentCandidate>, rusqlite::Error> {
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

pub fn apply_external_subjects(conn: &mut Connection, content_hash: ContentHash, provider_id: &str, isbn: &str, subjects: Vec<PublicationSubject>, status: &str, detail: Option<&str>) -> Result<usize, rusqlite::Error> {
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
        Err(sync_common::wire::WireError::UnsupportedVersion(version @ (10 | 11))) if raw.starts_with(b"BKHM") && raw.get(4) == Some(&version) => {
            let mut compatible = raw.to_vec();
            compatible[4] = sync_common::wire::header_bytes()[4];
            let metadata = sync_common::wire::decode::<PublicationMetadata>(&compatible, sync_common::wire::MAX_DECODED_REQUEST_BYTES).map_err(|error| invalid_text_column(0, error.to_string()))?;
            let reencoded = sync_common::wire::encode(&metadata).map_err(|error| invalid_text_column(0, error.to_string()))?;
            if reencoded.get(5..) != raw.get(5..) {
                return Err(invalid_text_column(0, format!("wire version {version} publication payload is not byte-compatible with the current wire version")));
            }
            log::info!("upgrading a byte-compatible publication metadata value from wire version {version}");
            Ok(metadata)
        }
        Err(error) => Err(invalid_text_column(0, error.to_string())),
    }
}
