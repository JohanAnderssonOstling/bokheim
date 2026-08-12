use catalog_contract::{CatalogPage, CatalogPublication, CatalogSourceId};
use catalog_model::normalize_search_text;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use uuid::Uuid;

use catalog_contract::{materialize_page, page_layout, CatalogPageResponse, CatalogPublicationPayload, CatalogPublicationReference, CatalogSearchResponse};

const CATALOG_CACHE_SCHEMA_VERSION: i64 = 1;

const CREATE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS catalog_source (
    source_id TEXT PRIMARY KEY,
    source_url TEXT NOT NULL,
    epoch TEXT NOT NULL,
    provider_epoch TEXT,
    active_generation INTEGER NOT NULL,
    next_sequence INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS catalog_page (
    source_id TEXT NOT NULL REFERENCES catalog_source(source_id) ON DELETE CASCADE,
    generation INTEGER NOT NULL,
    locator TEXT NOT NULL,
    payload BLOB NOT NULL,
    etag TEXT,
    last_modified TEXT,
    PRIMARY KEY(source_id, generation, locator)
);

CREATE TABLE IF NOT EXISTS catalog_publication (
    source_id TEXT NOT NULL REFERENCES catalog_source(source_id) ON DELETE CASCADE,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    payload_hash TEXT NOT NULL,
    normalized_title TEXT NOT NULL,
    payload BLOB NOT NULL,
    PRIMARY KEY(source_id, generation, edition_id, work_id)
);

CREATE TABLE IF NOT EXISTS catalog_publication_author (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    normalized_name TEXT NOT NULL,
    PRIMARY KEY(source_id, generation, edition_id, work_id, position),
    FOREIGN KEY(source_id, generation, edition_id, work_id)
        REFERENCES catalog_publication(source_id, generation, edition_id, work_id)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS catalog_publication_language (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    language TEXT NOT NULL,
    PRIMARY KEY(source_id, generation, edition_id, work_id, language),
    FOREIGN KEY(source_id, generation, edition_id, work_id)
        REFERENCES catalog_publication(source_id, generation, edition_id, work_id)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS catalog_publication_title
    ON catalog_publication(source_id, generation, normalized_title, edition_id, work_id);
CREATE INDEX IF NOT EXISTS catalog_publication_author_name
    ON catalog_publication_author(source_id, generation, normalized_name, edition_id, work_id);
CREATE INDEX IF NOT EXISTS catalog_publication_language_tag
    ON catalog_publication_language(source_id, generation, language, edition_id, work_id);
"#;

#[derive(Debug)]
pub struct CatalogCacheError(String);

impl CatalogCacheError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CatalogCacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CatalogCacheError {}

impl From<rusqlite::Error> for CatalogCacheError {
    fn from(error: rusqlite::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl From<r2d2::Error> for CatalogCacheError {
    fn from(error: r2d2::Error) -> Self {
        Self::new(error.to_string())
    }
}

impl From<wire::WireError> for CatalogCacheError {
    fn from(error: wire::WireError) -> Self {
        Self::new(error.to_string())
    }
}

#[derive(Clone)]
pub struct CatalogCache {
    pool: Pool<SqliteConnectionManager>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedCatalogPage {
    pub page: CatalogPage,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequencedCatalogPublication {
    pub publication: CatalogPublication,
    pub sequence: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogSearchMatches {
    pub title_matches: Vec<SequencedCatalogPublication>,
    pub author_matches: Vec<SequencedCatalogPublication>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSourceState {
    pub epoch: String,
    pub provider_epoch: Option<String>,
    pub active_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogCrawlGeneration {
    pub source_id: CatalogSourceId,
    pub generation: u64,
}

impl CatalogCache {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, CatalogCacheError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| CatalogCacheError::new(error.to_string()))?;
        }
        prepare_database_file(&path)?;
        let manager = SqliteConnectionManager::file(&path).with_init(configure_connection);
        let pool = Pool::builder().max_size(8).build(manager)?;
        {
            let connection = pool.get()?;
            connection.execute_batch(CREATE_SCHEMA)?;
            connection.pragma_update(None, "user_version", CATALOG_CACHE_SCHEMA_VERSION)?;
        }
        Ok(Self { pool })
    }

    pub fn source_state(&self, source_id: &CatalogSourceId) -> Result<Option<CatalogSourceState>, CatalogCacheError> {
        let connection = self.pool.get()?;
        connection
            .query_row("SELECT epoch, provider_epoch, active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| {
                Ok(CatalogSourceState { epoch: row.get(0)?, provider_epoch: row.get(1)?, active_generation: stored_u64(row.get(2)?, "active generation")? })
            })
            .optional()
            .map_err(Into::into)
    }

    /// Ensures a source exists. A URL change invalidates every canonical row
    /// for that source and creates a new epoch.
    pub fn ensure_source(&self, source_id: &CatalogSourceId, source_url: &str) -> Result<CatalogSourceState, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction()?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let state = source_state_in(&transaction, source_id)?.expect("source inserted");
        transaction.commit()?;
        Ok(state)
    }

    pub fn set_provider_epoch(&self, source_id: &CatalogSourceId, source_url: &str, provider_epoch: &str) -> Result<bool, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction()?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let previous = transaction.query_row("SELECT provider_epoch FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| row.get::<_, Option<String>>(0))?;
        let changed = previous.as_deref().is_some_and(|previous| previous != provider_epoch);
        if changed {
            clear_source_rows(&transaction, source_id)?;
        }
        transaction.execute("UPDATE catalog_source SET provider_epoch=?2 WHERE source_id=?1", params![source_id.as_str(), provider_epoch])?;
        transaction.commit()?;
        Ok(changed)
    }

    pub fn remove_source(&self, source_id: &CatalogSourceId) -> Result<(), CatalogCacheError> {
        let connection = self.pool.get()?;
        connection.execute("DELETE FROM catalog_source WHERE source_id=?1", [source_id.as_str()])?;
        Ok(())
    }

    /// Stores a directly fetched page into the active local generation.
    pub fn put_page(&self, source_id: &CatalogSourceId, source_url: &str, page: &CatalogPage, etag: Option<&str>, last_modified: Option<&str>) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction()?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let generation = active_generation_in(&transaction, source_id)?;
        put_page_row(&transaction, source_id, generation, page, etag, last_modified)?;
        for publication in unique_page_publications(page) {
            upsert_active_publication(&transaction, source_id, generation, publication)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn page(&self, source_id: &CatalogSourceId, locator: &str) -> Result<Option<CachedCatalogPage>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else {
            return Ok(None);
        };
        connection
            .query_row("SELECT payload, etag, last_modified FROM catalog_page WHERE source_id=?1 AND generation=?2 AND locator=?3", params![source_id.as_str(), storage_i64(generation, "generation")?, locator], |row| {
                let payload: Vec<u8> = row.get(0)?;
                let page = decode_page(&payload).map_err(sql_decode_error)?;
                Ok(CachedCatalogPage { page, etag: row.get(1)?, last_modified: row.get(2)? })
            })
            .optional()
            .map_err(Into::into)
    }

    pub fn pages(&self, source_id: &CatalogSourceId) -> Result<Vec<CachedCatalogPage>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else {
            return Ok(Vec::new());
        };
        let mut statement = connection.prepare(
            "SELECT payload, etag, last_modified FROM catalog_page
             WHERE source_id=?1 AND generation=?2 ORDER BY locator",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), storage_i64(generation, "generation")?], |row| {
            let payload: Vec<u8> = row.get(0)?;
            let page = decode_page(&payload).map_err(sql_decode_error)?;
            Ok(CachedCatalogPage { page, etag: row.get(1)?, last_modified: row.get(2)? })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn known_page_publications(&self, source_id: &CatalogSourceId, locator: &str) -> Result<Vec<CatalogPublicationReference>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else {
            return Ok(Vec::new());
        };
        let Some(page) = page_in(&connection, source_id, generation, locator)? else {
            return Ok(Vec::new());
        };
        let mut sequence = connection.prepare_cached(
            "SELECT sequence FROM catalog_publication
             WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
        )?;
        unique_page_publications(&page.page)
            .into_iter()
            .map(|publication| {
                let stored = sequence
                    .query_row(params![source_id.as_str(), storage_i64(generation, "generation")?, publication.identity.edition_id, publication.identity.work_id.as_deref().unwrap_or("")], |row| row.get::<_, i64>(0))
                    .optional()?
                    .ok_or_else(|| CatalogCacheError::new("catalog page references a missing publication"))?;
                Ok(CatalogPublicationReference { identity: publication.identity.clone(), sequence: stored_u64(stored, "publication sequence")? })
            })
            .collect()
    }

    pub fn page_response(&self, source_id: &CatalogSourceId, locator: &str, request_epoch: Option<&str>, known: &[CatalogPublicationReference]) -> Result<Option<CatalogPageResponse>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(state) = source_state_connection(&connection, source_id)? else {
            return Ok(None);
        };
        let Some(page) = page_in(&connection, source_id, state.active_generation, locator)? else {
            return Ok(None);
        };
        let known = if request_epoch == Some(state.epoch.as_str()) { known.iter().map(|item| (item.identity.clone(), item.sequence)).collect::<HashMap<_, _>>() } else { HashMap::new() };
        let mut payloads = Vec::new();
        let mut references = HashMap::new();
        for publication in unique_page_publications(&page.page) {
            let sequenced = sequenced_publication_in(&connection, source_id, state.active_generation, &publication.identity)?.ok_or_else(|| CatalogCacheError::new("catalog page references a missing publication"))?;
            let reference = CatalogPublicationReference { identity: publication.identity.clone(), sequence: sequenced.sequence };
            if known.get(&reference.identity) != Some(&reference.sequence) {
                payloads.push(CatalogPublicationPayload { publication: sequenced.publication, sequence: reference.sequence });
            }
            references.insert(reference.identity.clone(), reference);
        }
        let layout = page_layout(&page.page, |publication| references[&publication.identity].clone());
        Ok(Some(CatalogPageResponse { epoch: state.epoch, page: layout, payloads }))
    }

    pub fn apply_page_response(&self, source_url: &str, response: CatalogPageResponse) -> Result<CatalogPage, CatalogCacheError> {
        self.set_provider_epoch(&response.page.source_id, source_url, &response.epoch)?;
        let connection = self.pool.get()?;
        let state = source_state_connection(&connection, &response.page.source_id)?.ok_or_else(|| CatalogCacheError::new("catalog source disappeared"))?;
        let supplied = response.payloads.iter().map(|payload| (payload.publication.identity.clone(), payload.clone())).collect::<HashMap<_, _>>();
        let mut resolved = HashMap::new();
        for reference in response.page.publications.iter().chain(response.page.groups.iter().flat_map(|group| group.publications.iter())) {
            if resolved.contains_key(&reference.identity) {
                continue;
            }
            if let Some(payload) = supplied.get(&reference.identity).filter(|payload| payload.sequence == reference.sequence) {
                resolved.insert(reference.identity.clone(), payload.publication.clone());
                continue;
            }
            let cached = sequenced_publication_in(&connection, &response.page.source_id, state.active_generation, &reference.identity)?
                .filter(|publication| publication.sequence == reference.sequence)
                .ok_or_else(|| CatalogCacheError::new("catalog server omitted a required publication payload"))?;
            resolved.insert(reference.identity.clone(), cached.publication);
        }
        drop(connection);
        let page = materialize_page(response.page, |reference| resolved.get(&reference.identity).cloned()).ok_or_else(|| CatalogCacheError::new("catalog page could not be reconstructed"))?;
        self.put_page_with_sequences(&page.source_id, source_url, &page, &response.payloads)?;
        Ok(page)
    }

    pub fn search_response(&self, source_id: &CatalogSourceId, query: &str, request_epoch: Option<&str>, known: &[CatalogPublicationReference]) -> Result<Option<CatalogSearchResponse>, CatalogCacheError> {
        let Some(state) = self.source_state(source_id)? else {
            return Ok(None);
        };
        let matches = self.search(source_id, query)?;
        let known = if request_epoch == Some(state.epoch.as_str()) { known.iter().map(|item| (item.identity.clone(), item.sequence)).collect::<HashMap<_, _>>() } else { HashMap::new() };
        let title_matches = matches.title_matches.iter().map(publication_reference).collect::<Vec<_>>();
        let author_matches = matches.author_matches.iter().map(publication_reference).collect::<Vec<_>>();
        let payloads = matches
            .title_matches
            .into_iter()
            .chain(matches.author_matches)
            .filter(|publication| known.get(&publication.publication.identity) != Some(&publication.sequence))
            .map(|publication| CatalogPublicationPayload { publication: publication.publication, sequence: publication.sequence })
            .collect();
        Ok(Some(CatalogSearchResponse { source_id: source_id.clone(), epoch: state.epoch, query: query.to_owned(), title_matches, author_matches, payloads }))
    }

    pub fn apply_search_response(&self, source_url: &str, response: CatalogSearchResponse) -> Result<CatalogSearchMatches, CatalogCacheError> {
        self.set_provider_epoch(&response.source_id, source_url, &response.epoch)?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction()?;
        ensure_source_in(&transaction, &response.source_id, source_url)?;
        let generation = active_generation_in(&transaction, &response.source_id)?;
        for payload in &response.payloads {
            upsert_publication_with_sequence(&transaction, &response.source_id, generation, &payload.publication, payload.sequence)?;
        }
        let resolve = |reference: &CatalogPublicationReference| -> Result<SequencedCatalogPublication, CatalogCacheError> {
            let publication = transaction
                .query_row(
                    "SELECT payload, sequence FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
                    params![response.source_id.as_str(), storage_i64(generation, "generation")?, reference.identity.edition_id, reference.identity.work_id.as_deref().unwrap_or("")],
                    decode_sequenced_publication,
                )
                .optional()?
                .filter(|publication| publication.sequence == reference.sequence)
                .ok_or_else(|| CatalogCacheError::new("catalog server omitted a required search publication payload"))?;
            Ok(publication)
        };
        let title_matches = response.title_matches.iter().map(&resolve).collect::<Result<Vec<_>, _>>()?;
        let author_matches = response.author_matches.iter().map(&resolve).collect::<Result<Vec<_>, _>>()?;
        transaction.commit()?;
        Ok(CatalogSearchMatches { title_matches, author_matches })
    }

    pub fn search(&self, source_id: &CatalogSourceId, query: &str) -> Result<CatalogSearchMatches, CatalogCacheError> {
        let normalized_query = normalize_search_text(query);
        if normalized_query.is_empty() {
            return Ok(CatalogSearchMatches::default());
        }
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else {
            return Ok(CatalogSearchMatches::default());
        };
        let generation = storage_i64(generation, "generation")?;
        let mut title_matches = Vec::new();
        let mut statement = connection.prepare(
            "SELECT payload, sequence FROM catalog_publication
             WHERE source_id=?1 AND generation=?2 AND instr(normalized_title, ?3)>0
             ORDER BY normalized_title, edition_id, work_id",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), generation, normalized_query], decode_sequenced_publication)?;
        for row in rows {
            title_matches.push(row?);
        }

        let mut author_matches = Vec::new();
        let mut statement = connection.prepare(
            "SELECT publication.payload, publication.sequence
             FROM catalog_publication publication
             WHERE publication.source_id=?1 AND publication.generation=?2
               AND instr(publication.normalized_title, ?3)=0
               AND EXISTS (
                   SELECT 1 FROM catalog_publication_author author
                   WHERE author.source_id=publication.source_id
                     AND author.generation=publication.generation
                     AND author.edition_id=publication.edition_id
                     AND author.work_id=publication.work_id
                     AND instr(author.normalized_name, ?3)>0
               )
             ORDER BY publication.normalized_title, publication.edition_id, publication.work_id",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), generation, normalized_query], decode_sequenced_publication)?;
        for row in rows {
            author_matches.push(row?);
        }
        Ok(CatalogSearchMatches { title_matches, author_matches })
    }

    pub fn begin_crawl(&self, source_id: &CatalogSourceId, source_url: &str) -> Result<CatalogCrawlGeneration, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction()?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let active = active_generation_in(&transaction, source_id)?;
        let generation = active.checked_add(1).ok_or_else(|| CatalogCacheError::new("catalog generation overflow"))?;
        transaction.execute("DELETE FROM catalog_page WHERE source_id=?1 AND generation=?2", params![source_id.as_str(), storage_i64(generation, "generation")?])?;
        transaction.execute("DELETE FROM catalog_publication WHERE source_id=?1 AND generation=?2", params![source_id.as_str(), storage_i64(generation, "generation")?])?;
        transaction.commit()?;
        Ok(CatalogCrawlGeneration { source_id: source_id.clone(), generation })
    }

    pub fn stage_page(&self, crawl: &CatalogCrawlGeneration, page: &CatalogPage, etag: Option<&str>, last_modified: Option<&str>) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction()?;
        put_page_row(&transaction, &crawl.source_id, crawl.generation, page, etag, last_modified)?;
        for publication in unique_page_publications(page) {
            insert_staged_publication(&transaction, &crawl.source_id, crawl.generation, publication)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn activate_crawl(&self, crawl: &CatalogCrawlGeneration) -> Result<CatalogSourceState, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction()?;
        let active = active_generation_in(&transaction, &crawl.source_id)?;
        let mut next_sequence = next_sequence_in(&transaction, &crawl.source_id)?;
        let staged = staged_publication_keys(&transaction, crawl)?;
        for (edition_id, work_id, payload_hash) in staged {
            let old_sequence = transaction
                .query_row(
                    "SELECT sequence FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 AND payload_hash=?5",
                    params![crawl.source_id.as_str(), storage_i64(active, "generation")?, edition_id, work_id, payload_hash],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            let sequence = match old_sequence {
                Some(sequence) => sequence,
                None => {
                    next_sequence = next_sequence.checked_add(1).ok_or_else(|| CatalogCacheError::new("publication sequence overflow"))?;
                    storage_i64(next_sequence, "publication sequence")?
                }
            };
            transaction.execute(
                "UPDATE catalog_publication SET sequence=?5 WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
                params![crawl.source_id.as_str(), storage_i64(crawl.generation, "generation")?, edition_id, work_id, sequence],
            )?;
        }
        transaction.execute(
            "UPDATE catalog_source SET active_generation=?2, next_sequence=?3 WHERE source_id=?1",
            params![crawl.source_id.as_str(), storage_i64(crawl.generation, "generation")?, storage_i64(next_sequence, "publication sequence")?],
        )?;
        transaction.execute("DELETE FROM catalog_page WHERE source_id=?1 AND generation<>?2", params![crawl.source_id.as_str(), storage_i64(crawl.generation, "generation")?])?;
        transaction.execute("DELETE FROM catalog_publication WHERE source_id=?1 AND generation<>?2", params![crawl.source_id.as_str(), storage_i64(crawl.generation, "generation")?])?;
        let state = source_state_in(&transaction, &crawl.source_id)?.expect("crawl source exists");
        transaction.commit()?;
        Ok(state)
    }

    pub fn abandon_crawl(&self, crawl: &CatalogCrawlGeneration) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM catalog_page WHERE source_id=?1 AND generation=?2", params![crawl.source_id.as_str(), storage_i64(crawl.generation, "generation")?])?;
        transaction.execute("DELETE FROM catalog_publication WHERE source_id=?1 AND generation=?2", params![crawl.source_id.as_str(), storage_i64(crawl.generation, "generation")?])?;
        transaction.commit()?;
        Ok(())
    }

    fn put_page_with_sequences(&self, source_id: &CatalogSourceId, source_url: &str, page: &CatalogPage, payloads: &[CatalogPublicationPayload]) -> Result<(), CatalogCacheError> {
        let supplied = payloads.iter().map(|payload| (payload.publication.identity.clone(), payload.sequence)).collect::<HashMap<_, _>>();
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction()?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let generation = active_generation_in(&transaction, source_id)?;
        put_page_row(&transaction, source_id, generation, page, None, None)?;
        for publication in unique_page_publications(page) {
            let sequence =
                supplied.get(&publication.identity).copied().or_else(|| sequence_in(&transaction, source_id, generation, &publication.identity).ok().flatten()).ok_or_else(|| CatalogCacheError::new("publication sequence is unavailable"))?;
            upsert_publication_with_sequence(&transaction, source_id, generation, publication, sequence)?;
        }
        transaction.commit()?;
        Ok(())
    }
}

fn prepare_database_file(path: &Path) -> Result<(), CatalogCacheError> {
    if !path.exists() {
        return Ok(());
    }
    let usable = Connection::open(path).and_then(|connection| connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))).is_ok_and(|version| version == CATALOG_CACHE_SCHEMA_VERSION);
    if !usable {
        fs::remove_file(path).map_err(|error| CatalogCacheError::new(error.to_string()))?;
        for suffix in ["-wal", "-shm"] {
            let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
            if sidecar.exists() {
                fs::remove_file(sidecar).map_err(|error| CatalogCacheError::new(error.to_string()))?;
            }
        }
    }
    Ok(())
}

fn configure_connection(connection: &mut Connection) -> rusqlite::Result<()> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.busy_timeout(Duration::from_secs(10))
}

fn ensure_source_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId, source_url: &str) -> Result<(), CatalogCacheError> {
    let existing = transaction.query_row("SELECT source_url FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| row.get::<_, String>(0)).optional()?;
    match existing {
        Some(existing) if existing == source_url => {}
        Some(_) => {
            transaction.execute("DELETE FROM catalog_source WHERE source_id=?1", [source_id.as_str()])?;
            insert_source(transaction, source_id, source_url)?;
        }
        None => insert_source(transaction, source_id, source_url)?,
    }
    Ok(())
}

fn insert_source(transaction: &Transaction<'_>, source_id: &CatalogSourceId, source_url: &str) -> Result<(), CatalogCacheError> {
    transaction.execute("INSERT INTO catalog_source(source_id, source_url, epoch, provider_epoch, active_generation, next_sequence) VALUES(?1, ?2, ?3, NULL, 1, 0)", params![source_id.as_str(), source_url, Uuid::new_v4().to_string()])?;
    Ok(())
}

fn clear_source_rows(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<(), CatalogCacheError> {
    transaction.execute("DELETE FROM catalog_page WHERE source_id=?1", [source_id.as_str()])?;
    transaction.execute("DELETE FROM catalog_publication WHERE source_id=?1", [source_id.as_str()])?;
    transaction.execute("UPDATE catalog_source SET epoch=?2, active_generation=1, next_sequence=0 WHERE source_id=?1", params![source_id.as_str(), Uuid::new_v4().to_string()])?;
    Ok(())
}

fn source_state_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<Option<CatalogSourceState>, CatalogCacheError> {
    transaction
        .query_row("SELECT epoch, provider_epoch, active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| {
            Ok(CatalogSourceState { epoch: row.get(0)?, provider_epoch: row.get(1)?, active_generation: stored_u64(row.get(2)?, "active generation")? })
        })
        .optional()
        .map_err(Into::into)
}

fn source_state_connection(connection: &Connection, source_id: &CatalogSourceId) -> Result<Option<CatalogSourceState>, CatalogCacheError> {
    connection
        .query_row("SELECT epoch, provider_epoch, active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| {
            Ok(CatalogSourceState { epoch: row.get(0)?, provider_epoch: row.get(1)?, active_generation: stored_u64(row.get(2)?, "active generation")? })
        })
        .optional()
        .map_err(Into::into)
}

fn active_generation_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<u64, CatalogCacheError> {
    transaction.query_row("SELECT active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| stored_u64(row.get(0)?, "active generation")).map_err(Into::into)
}

fn active_generation_optional(connection: &Connection, source_id: &CatalogSourceId) -> Result<Option<u64>, CatalogCacheError> {
    connection.query_row("SELECT active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| stored_u64(row.get(0)?, "active generation")).optional().map_err(Into::into)
}

fn next_sequence_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<u64, CatalogCacheError> {
    transaction.query_row("SELECT next_sequence FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| stored_u64(row.get(0)?, "next sequence")).map_err(Into::into)
}

fn put_page_row(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, page: &CatalogPage, etag: Option<&str>, last_modified: Option<&str>) -> Result<(), CatalogCacheError> {
    let payload = wire::encode(page)?;
    transaction.execute(
        "INSERT INTO catalog_page(source_id, generation, locator, payload, etag, last_modified) VALUES(?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(source_id, generation, locator) DO UPDATE SET payload=excluded.payload, etag=excluded.etag, last_modified=excluded.last_modified",
        params![source_id.as_str(), storage_i64(generation, "generation")?, page.locator, payload, etag, last_modified],
    )?;
    Ok(())
}

fn page_in(connection: &Connection, source_id: &CatalogSourceId, generation: u64, locator: &str) -> Result<Option<CachedCatalogPage>, CatalogCacheError> {
    connection
        .query_row("SELECT payload, etag, last_modified FROM catalog_page WHERE source_id=?1 AND generation=?2 AND locator=?3", params![source_id.as_str(), storage_i64(generation, "generation")?, locator], |row| {
            let payload: Vec<u8> = row.get(0)?;
            let page = decode_page(&payload).map_err(sql_decode_error)?;
            Ok(CachedCatalogPage { page, etag: row.get(1)?, last_modified: row.get(2)? })
        })
        .optional()
        .map_err(Into::into)
}

fn unique_page_publications(page: &CatalogPage) -> Vec<&CatalogPublication> {
    let mut seen = HashSet::new();
    page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).filter(|publication| seen.insert((publication.identity.edition_id.as_str(), publication.identity.work_id.as_deref()))).collect()
}

fn publication_key(publication: &CatalogPublication) -> (&str, &str) {
    (&publication.identity.edition_id, publication.identity.work_id.as_deref().unwrap_or(""))
}

fn upsert_active_publication(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, publication: &CatalogPublication) -> Result<(), CatalogCacheError> {
    let payload = wire::encode(publication)?;
    let payload_hash = blake3::hash(&payload).to_hex().to_string();
    let (edition_id, work_id) = publication_key(publication);
    let previous = transaction
        .query_row(
            "SELECT payload_hash, sequence FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, edition_id, work_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;
    let sequence = if let Some((previous_hash, sequence)) = previous {
        if previous_hash == payload_hash {
            sequence
        } else {
            allocate_sequence(transaction, source_id)?
        }
    } else {
        allocate_sequence(transaction, source_id)?
    };
    transaction.execute(
        "INSERT INTO catalog_publication(source_id, generation, edition_id, work_id, sequence, payload_hash, normalized_title, payload)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(source_id, generation, edition_id, work_id) DO UPDATE SET sequence=excluded.sequence, payload_hash=excluded.payload_hash, normalized_title=excluded.normalized_title, payload=excluded.payload",
        params![source_id.as_str(), storage_i64(generation, "generation")?, edition_id, work_id, sequence, payload_hash, normalize_search_text(&publication.title), payload],
    )?;
    replace_publication_metadata(transaction, source_id, generation, publication)?;
    Ok(())
}

fn upsert_publication_with_sequence(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, publication: &CatalogPublication, sequence: u64) -> Result<(), CatalogCacheError> {
    let payload = wire::encode(publication)?;
    let payload_hash = blake3::hash(&payload).to_hex().to_string();
    let (edition_id, work_id) = publication_key(publication);
    let sequence = storage_i64(sequence, "publication sequence")?;
    transaction.execute(
        "INSERT INTO catalog_publication(source_id, generation, edition_id, work_id, sequence, payload_hash, normalized_title, payload)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(source_id, generation, edition_id, work_id) DO UPDATE SET sequence=excluded.sequence, payload_hash=excluded.payload_hash, normalized_title=excluded.normalized_title, payload=excluded.payload",
        params![source_id.as_str(), storage_i64(generation, "generation")?, edition_id, work_id, sequence, payload_hash, normalize_search_text(&publication.title), payload],
    )?;
    transaction.execute("UPDATE catalog_source SET next_sequence=MAX(next_sequence, ?2) WHERE source_id=?1", params![source_id.as_str(), sequence])?;
    replace_publication_metadata(transaction, source_id, generation, publication)
}

fn insert_staged_publication(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, publication: &CatalogPublication) -> Result<(), CatalogCacheError> {
    let payload = wire::encode(publication)?;
    let payload_hash = blake3::hash(&payload).to_hex().to_string();
    let (edition_id, work_id) = publication_key(publication);
    let inserted = transaction.execute(
        "INSERT OR IGNORE INTO catalog_publication(source_id, generation, edition_id, work_id, sequence, payload_hash, normalized_title, payload)
         VALUES(?1, ?2, ?3, ?4, 0, ?5, ?6, ?7)",
        params![source_id.as_str(), storage_i64(generation, "generation")?, edition_id, work_id, payload_hash, normalize_search_text(&publication.title), payload],
    )?;
    if inserted > 0 {
        replace_publication_metadata(transaction, source_id, generation, publication)?;
    }
    Ok(())
}

fn replace_publication_metadata(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, publication: &CatalogPublication) -> Result<(), CatalogCacheError> {
    let (edition_id, work_id) = publication_key(publication);
    let generation = storage_i64(generation, "generation")?;
    transaction.execute("DELETE FROM catalog_publication_author WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, edition_id, work_id])?;
    for (position, author) in publication.authors.iter().enumerate() {
        transaction.execute(
            "INSERT INTO catalog_publication_author(source_id, generation, edition_id, work_id, position, normalized_name) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
            params![source_id.as_str(), generation, edition_id, work_id, i64::try_from(position).map_err(|_| CatalogCacheError::new("author position overflow"))?, normalize_search_text(&author.name)],
        )?;
    }
    transaction.execute("DELETE FROM catalog_publication_language WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, edition_id, work_id])?;
    for language in &publication.languages {
        transaction.execute("INSERT OR IGNORE INTO catalog_publication_language(source_id, generation, edition_id, work_id, language) VALUES(?1, ?2, ?3, ?4, ?5)", params![source_id.as_str(), generation, edition_id, work_id, language])?;
    }
    Ok(())
}

fn allocate_sequence(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<i64, CatalogCacheError> {
    let next = next_sequence_in(transaction, source_id)?.checked_add(1).ok_or_else(|| CatalogCacheError::new("publication sequence overflow"))?;
    transaction.execute("UPDATE catalog_source SET next_sequence=?2 WHERE source_id=?1", params![source_id.as_str(), storage_i64(next, "publication sequence")?])?;
    storage_i64(next, "publication sequence")
}

fn staged_publication_keys(transaction: &Transaction<'_>, crawl: &CatalogCrawlGeneration) -> Result<Vec<(String, String, String)>, CatalogCacheError> {
    let mut statement = transaction.prepare("SELECT edition_id, work_id, payload_hash FROM catalog_publication WHERE source_id=?1 AND generation=?2 ORDER BY edition_id, work_id")?;
    let rows = statement.query_map(params![crawl.source_id.as_str(), storage_i64(crawl.generation, "generation")?], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

fn decode_sequenced_publication(row: &rusqlite::Row<'_>) -> rusqlite::Result<SequencedCatalogPublication> {
    let payload: Vec<u8> = row.get(0)?;
    let publication = wire::decode(&payload, wire::MAX_DECODED_RESPONSE_BYTES).map_err(sql_decode_error)?;
    let sequence = stored_u64(row.get(1)?, "publication sequence")?;
    Ok(SequencedCatalogPublication { publication, sequence })
}

fn sequenced_publication_in(connection: &Connection, source_id: &CatalogSourceId, generation: u64, identity: &catalog_contract::CatalogPublicationIdentity) -> Result<Option<SequencedCatalogPublication>, CatalogCacheError> {
    connection
        .query_row(
            "SELECT payload, sequence FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, identity.edition_id, identity.work_id.as_deref().unwrap_or("")],
            decode_sequenced_publication,
        )
        .optional()
        .map_err(Into::into)
}

fn sequence_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, identity: &catalog_contract::CatalogPublicationIdentity) -> Result<Option<u64>, CatalogCacheError> {
    transaction
        .query_row(
            "SELECT sequence FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, identity.edition_id, identity.work_id.as_deref().unwrap_or("")],
            |row| stored_u64(row.get(0)?, "publication sequence"),
        )
        .optional()
        .map_err(Into::into)
}

fn publication_reference(publication: &SequencedCatalogPublication) -> CatalogPublicationReference {
    CatalogPublicationReference { identity: publication.publication.identity.clone(), sequence: publication.sequence }
}

fn decode_page(payload: &[u8]) -> Result<CatalogPage, wire::WireError> {
    wire::decode(payload, wire::MAX_DECODED_RESPONSE_BYTES)
}

fn sql_decode_error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(error))
}

fn storage_i64(value: u64, name: &str) -> Result<i64, CatalogCacheError> {
    i64::try_from(value).map_err(|_| CatalogCacheError::new(format!("{name} exceeds SQLite integer range")))
}

fn stored_u64(value: i64, name: &str) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Integer, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{name} is negative")))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog_contract::{CatalogAuthorCredit, CatalogFreshness, CatalogPagination, CatalogPublicationIdentity};

    fn source_id() -> CatalogSourceId {
        CatalogSourceId::parse("opds-test").unwrap()
    }

    fn publication(id: &str, title: &str, author: &str, languages: &[&str]) -> CatalogPublication {
        CatalogPublication {
            identity: CatalogPublicationIdentity { edition_id: id.to_owned(), work_id: None },
            title: title.to_owned(),
            authors: vec![CatalogAuthorCredit { name: author.to_owned(), sort_name: None, identifiers: Vec::new(), provider_link: None }],
            languages: languages.iter().map(|language| (*language).to_owned()).collect(),
            description: None,
            image_url: None,
            actions: Vec::new(),
        }
    }

    fn page(locator: &str, publications: Vec<CatalogPublication>) -> CatalogPage {
        CatalogPage {
            source_id: source_id(),
            freshness: CatalogFreshness::Current,
            title: "Books".to_owned(),
            locator: locator.to_owned(),
            navigation: Vec::new(),
            publications,
            groups: Vec::new(),
            facets: Vec::new(),
            pagination: CatalogPagination::default(),
            search: None,
        }
    }

    #[test]
    fn local_search_separates_title_and_author_matches() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        cache
            .put_page(
                &source_id(),
                "https://example.com/opds",
                &page("https://example.com/opds", vec![publication("title", "Cien años de soledad", "Someone", &["es"]), publication("author", "Chronicle", "Gabriel García Márquez", &["es"])]),
                None,
                None,
            )
            .unwrap();
        let matches = cache.search(&source_id(), "garcia marquez").unwrap();
        assert!(matches.title_matches.is_empty());
        assert_eq!(matches.author_matches[0].publication.identity.edition_id, "author");
        let matches = cache.search(&source_id(), "cien anos").unwrap();
        assert_eq!(matches.title_matches[0].publication.identity.edition_id, "title");
        assert!(matches.author_matches.is_empty());
    }

    #[test]
    fn failed_staging_keeps_active_generation_and_activation_reuses_unchanged_sequences() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        let first = page("https://example.com/opds", vec![publication("one", "One", "Author", &["en"]), publication("two", "Two", "Author", &["en"])]);
        cache.put_page(&source_id(), "https://example.com/opds", &first, Some("v1"), None).unwrap();
        let original = cache.search(&source_id(), "one").unwrap().title_matches[0].sequence;

        let failed = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
        cache.stage_page(&failed, &page("https://example.com/opds", vec![publication("replacement", "Replacement", "Author", &["en"])]), None, None).unwrap();
        cache.abandon_crawl(&failed).unwrap();
        assert_eq!(cache.search(&source_id(), "one").unwrap().title_matches[0].sequence, original);

        let successful = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
        cache.stage_page(&successful, &first, Some("v1"), None).unwrap();
        cache.activate_crawl(&successful).unwrap();
        assert_eq!(cache.search(&source_id(), "one").unwrap().title_matches[0].sequence, original);
    }

    #[test]
    fn source_url_and_provider_epoch_changes_invalidate_only_that_source() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        cache.put_page(&source_id(), "https://example.com/one", &page("https://example.com/one", vec![publication("one", "One", "Author", &["en"])]), None, None).unwrap();
        assert!(!cache.set_provider_epoch(&source_id(), "https://example.com/one", "server-a").unwrap());
        assert!(cache.set_provider_epoch(&source_id(), "https://example.com/one", "server-b").unwrap());
        assert!(cache.search(&source_id(), "one").unwrap().title_matches.is_empty());

        cache.put_page(&source_id(), "https://example.com/one", &page("https://example.com/one", vec![publication("one", "One", "Author", &["en"])]), None, None).unwrap();
        let old_epoch = cache.source_state(&source_id()).unwrap().unwrap().epoch;
        cache.ensure_source(&source_id(), "https://example.com/two").unwrap();
        let state = cache.source_state(&source_id()).unwrap().unwrap();
        assert_ne!(state.epoch, old_epoch);
        assert!(cache.search(&source_id(), "one").unwrap().title_matches.is_empty());
    }

    #[test]
    fn known_page_publications_returns_references_without_building_a_response() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        let locator = "https://example.com/opds";
        cache.put_page(&source_id(), locator, &page(locator, vec![publication("one", "One", "Author", &["en"]), publication("two", "Two", "Author", &["en"])]), None, None).unwrap();

        let references = cache.known_page_publications(&source_id(), locator).unwrap();
        assert_eq!(references.iter().map(|reference| reference.identity.edition_id.as_str()).collect::<Vec<_>>(), vec!["one", "two"]);
        assert!(references.iter().all(|reference| reference.sequence > 0));
        assert!(cache.known_page_publications(&source_id(), "https://example.com/missing").unwrap().is_empty());
    }
}
