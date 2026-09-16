//! Device-local resumable progress and bounded ISBN response cache.
use crate::storage_values::invalid_text_column;
use library_model::BookFormat;
use publication_enrichment::SubjectPipeline;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use sync_common::ContentHash;

// Invalidate unsuccessful checkpoints when local evidence rules change.
const INSPECTION_REVISION: i64 = 3;

pub struct Candidate {
    pub hash: ContentHash,
    pub title: String,
    pub format: BookFormat,
    pub publication: library_model::PublicationMetadata,
    pub pipeline: SubjectPipeline,
}

pub fn candidates(conn: &Connection, after: &str, limit: usize) -> rusqlite::Result<Vec<Candidate>> {
    candidates_scoped(conn, after, limit, None)
}

pub fn candidates_scoped(conn: &Connection, after: &str, limit: usize, scope: Option<&str>) -> rusqlite::Result<Vec<Candidate>> {
    let mut query = conn.prepare_cached(include_str!("sql/subject_pipeline/candidates_select.sql"))?;
    query
        .query_map(rusqlite::params![after, limit as i64, INSPECTION_REVISION, scope], |r| {
            let raw: Vec<u8> = r.get(3)?;
            let state: Option<String> = r.get(4)?;
            let extension: String = r.get(2)?;
            Ok(Candidate {
                hash: ContentHash::new(&r.get::<_, String>(0)?),
                title: r.get(1)?,
                format: BookFormat::from_extension(&extension).ok_or_else(|| invalid_text_column(2, "unknown book format"))?,
                publication: super::external_metadata::decode_publication_metadata(&raw)?,
                pipeline: state.map(|s| serde_json::from_str(&s).map_err(|e| invalid_text_column(4, e.to_string()))).transpose()?.unwrap_or_default(),
            })
        })?
        .collect()
}

pub fn publication(conn: &Connection, hash: &ContentHash) -> rusqlite::Result<library_model::PublicationMetadata> {
    let raw: Vec<u8> = conn.query_row(include_str!("sql/subject_pipeline/publication_select.sql"), [hash.as_str()], |r| r.get(0))?;
    super::external_metadata::decode_publication_metadata(&raw)
}

pub fn save(conn: &rusqlite::Transaction<'_>, hash: ContentHash, state: &SubjectPipeline, expected: &library_model::PublicationMetadata, title: &str, complete: bool, retry_seconds: i64) -> rusqlite::Result<()> {
    prepare_save(hash, state, expected, title, complete, retry_seconds)?.apply(conn)
}

pub(super) struct PreparedCheckpoint {
    hash: ContentHash,
    state: String,
    expected: Vec<u8>,
    title: String,
    complete: bool,
    retry_seconds: i64,
    empty_metadata: bool,
}
pub(super) fn prepare_save(hash: ContentHash, state: &SubjectPipeline, expected: &library_model::PublicationMetadata, title: &str, complete: bool, retry_seconds: i64) -> rusqlite::Result<PreparedCheckpoint> {
    let mut state = serde_json::to_value(state).map_err(|e| invalid_text_column(0, e.to_string()))?;
    state["inspection_revision"] = INSPECTION_REVISION.into();
    let state = state.to_string();
    let empty_metadata = expected == &library_model::PublicationMetadata::default();
    let expected = sync_common::wire::encode(expected).map_err(|e| invalid_text_column(0, e.to_string()))?;
    Ok(PreparedCheckpoint { hash, state, expected, title: title.to_owned(), complete, retry_seconds, empty_metadata })
}
impl PreparedCheckpoint {
    pub(super) fn apply(self, conn: &rusqlite::Transaction<'_>) -> rusqlite::Result<()> {
        let Self { hash, state, expected, title, complete, retry_seconds, empty_metadata } = self;
        conn.execute(include_str!("sql/subject_pipeline/apply_insert.sql"), rusqlite::params![hash.as_str(), state, complete, retry_seconds, expected, title, empty_metadata])?;
        Ok(())
    }
}

/// Merge evidence into the latest row, preserving concurrent edits and IDs.
pub fn merge_pages(conn: &rusqlite::Transaction<'_>, hash: ContentHash, evidence: &library_model::PublicationMetadata) -> rusqlite::Result<bool> {
    let mut current = publication(conn, &hash)?;
    let before = current.clone();
    for id in &evidence.identifiers {
        if !current.identifiers.contains(id) {
            current.identifiers.push(id.clone());
        }
    }
    for subject in &evidence.subjects {
        if !current.subjects.contains(subject) {
            current.subjects.push(subject.clone());
        }
    }
    for property in &evidence.properties {
        if !current.properties.contains(property) {
            current.properties.push(property.clone());
        }
    }
    if current == before {
        return Ok(false);
    }
    super::metadata::update_book_publication_metadata_preserving_ids(conn, hash, &current)?;
    Ok(true)
}

pub fn step_succeeded(conn: &Connection, hash: &ContentHash, provider: &str) -> rusqlite::Result<bool> {
    conn.query_row(include_str!("sql/subject_pipeline/step_succeeded_select.sql"), rusqlite::params![hash.as_str(), provider], |r| r.get(0)).optional().map(|v| v.unwrap_or(false))
}

pub fn cached_isbn(conn: &Connection, isbn: &str) -> rusqlite::Result<Option<metadata_contract::RichMetadataEnrichmentResponse>> {
    let key = publication_enrichment::isbn_key(isbn).unwrap_or_else(|| isbn.to_owned());
    let json: Option<String> = conn.query_row(include_str!("sql/subject_pipeline/cached_isbn_select.sql"), [&key], |r| r.get(0)).optional()?;
    // A stale/corrupt cache entry is safely fetched again.
    Ok(json.and_then(|v| serde_json::from_str::<metadata_contract::RichMetadataEnrichmentResponse>(&v).ok()).map(|mut response| {
        for result in &mut response.results {
            result.requested_isbn = isbn.to_owned();
        }
        response
    }))
}

pub fn cache_isbn(conn: &Connection, isbn: &str, response: &metadata_contract::RichMetadataEnrichmentResponse) -> rusqlite::Result<()> {
    let json = serde_json::to_string(response).map_err(|e| invalid_text_column(0, e.to_string()))?;
    cache_isbn_json(conn, isbn, &json)
}

pub fn cache_isbn_json(conn: &Connection, isbn: &str, json: &str) -> rusqlite::Result<()> {
    let key = publication_enrichment::isbn_key(isbn).unwrap_or_else(|| isbn.to_owned());
    conn.execute(include_str!("sql/subject_pipeline/cache_isbn_json_insert.sql"), rusqlite::params![key, json])?;
    conn.execute(include_str!("sql/subject_pipeline/cache_isbn_json_delete.sql"), [])?;
    Ok(())
}
