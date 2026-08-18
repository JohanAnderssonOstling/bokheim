use catalog_contract::{
    CatalogBookFormat, CatalogDescription, CatalogLink, CatalogLinkTarget, CatalogPage, CatalogPagination, CatalogPublication, CatalogPublicationAction, CatalogPublicationIdentity, CatalogRecommendation, CatalogRecommendationRequest,
    CatalogRecommendationResponse, CatalogSourceId, CatalogSourcedPublication, CatalogSubjectBrowseRequest, CatalogSubjectBrowseResponse, CatalogSubjectPreference,
};
use catalog_model::normalize_search_text;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

use catalog_contract::{materialize_page, page_layout, CatalogPageResponse, CatalogPublicationPayload, CatalogPublicationReference, CatalogSearchResponse};

// Keep SQLite's file version at 1 because the additive v2 schema remains
// writable by the previous binary during an automated deployment rollback.
// The logical version lives in catalog_cache_metadata instead.
const CATALOG_CACHE_FILE_VERSION: i64 = 1;
// Version 12 also invalidates payloads written with storage-wire version 11.
// The cache is rehydratable, so replacing it is safer than attempting to
// decode an incompatible persisted representation during startup.
const CATALOG_CACHE_SCHEMA_VERSION: i64 = 12;
const CATALOG_EPUB_ENRICHMENT_VERSION: i64 = 4;
const CATALOG_METADATA_BACKFILL_VERSION: i64 = 1;
const MAX_CRAWL_ERROR_BYTES: usize = 2_048;

const CREATE_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS catalog_source (
    source_id TEXT PRIMARY KEY,
    source_url TEXT NOT NULL,
    epoch TEXT NOT NULL,
    provider_epoch TEXT,
    active_generation INTEGER NOT NULL,
    next_sequence INTEGER NOT NULL,
    last_crawl_started_at INTEGER,
    last_crawl_succeeded_at INTEGER,
    crawl_status TEXT NOT NULL DEFAULT 'never',
    last_crawl_error TEXT,
    active_page_count INTEGER NOT NULL DEFAULT 0,
    active_publication_count INTEGER NOT NULL DEFAULT 0,
    active_epub_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS catalog_cache_metadata (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    schema_version INTEGER NOT NULL
);
INSERT OR IGNORE INTO catalog_cache_metadata(singleton, schema_version) VALUES(1, 12);

CREATE TABLE IF NOT EXISTS catalog_epub_extraction (
    source_id TEXT NOT NULL REFERENCES catalog_source(source_id) ON DELETE CASCADE,
    acquisition_url TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'succeeded', 'failed')),
    phase TEXT,
    attempts INTEGER NOT NULL DEFAULT 0,
    downloaded_bytes INTEGER NOT NULL DEFAULT 0,
    last_attempt_at INTEGER,
    last_succeeded_at INTEGER,
    last_error TEXT,
    result_payload BLOB,
    metadata_payload BLOB,
    enrichment_version INTEGER NOT NULL DEFAULT 3,
    edition_id TEXT,
    work_id TEXT,
    book_format TEXT,
    cover_key TEXT,
    scan_token TEXT,
    catalog_generation INTEGER,
    PRIMARY KEY(source_id, acquisition_url)
);

-- Additive diagnostics remain compatible with the previous cache writer.
CREATE TABLE IF NOT EXISTS catalog_epub_failure (
    source_id TEXT NOT NULL,
    acquisition_url TEXT NOT NULL,
    category TEXT NOT NULL,
    retryable INTEGER NOT NULL CHECK(retryable IN (0, 1)),
    first_failed_at INTEGER NOT NULL,
    last_failed_at INTEGER NOT NULL,
    next_retry_at INTEGER,
    PRIMARY KEY(source_id, acquisition_url),
    FOREIGN KEY(source_id, acquisition_url)
        REFERENCES catalog_epub_extraction(source_id, acquisition_url)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS catalog_metadata_backfill (
    source_id TEXT NOT NULL,
    acquisition_url TEXT NOT NULL,
    version INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'succeeded')),
    attempts INTEGER NOT NULL DEFAULT 0,
    last_attempt_at INTEGER,
    last_error TEXT,
    PRIMARY KEY(source_id, acquisition_url),
    FOREIGN KEY(source_id, acquisition_url)
        REFERENCES catalog_epub_extraction(source_id, acquisition_url)
        ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS catalog_metadata_backfill_status
    ON catalog_metadata_backfill(status, attempts, source_id, acquisition_url);

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
    book_format TEXT,
    importable INTEGER NOT NULL DEFAULT 0 CHECK(importable IN (0, 1)),
    has_upstream_cover INTEGER NOT NULL DEFAULT 0 CHECK(has_upstream_cover IN (0, 1)),
    inventory_indexed INTEGER NOT NULL DEFAULT 0 CHECK(inventory_indexed IN (0, 1)),
    acquisition_url TEXT,
    extraction_format TEXT,
    cover_url TEXT,
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

CREATE TABLE IF NOT EXISTS catalog_publication_subject (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    name TEXT NOT NULL,
    source TEXT NOT NULL,
    authority TEXT,
    code TEXT,
    PRIMARY KEY(source_id, generation, edition_id, work_id, position),
    FOREIGN KEY(source_id, generation, edition_id, work_id)
        REFERENCES catalog_publication(source_id, generation, edition_id, work_id)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS catalog_publication_subject_assignment (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    system_id TEXT NOT NULL,
    subject_path TEXT NOT NULL,
    matcher_version INTEGER NOT NULL,
    PRIMARY KEY(source_id, generation, edition_id, work_id, system_id, subject_path),
    FOREIGN KEY(source_id, generation, edition_id, work_id)
        REFERENCES catalog_publication(source_id, generation, edition_id, work_id)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS catalog_publication_toc (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    normalized_title TEXT NOT NULL,
    PRIMARY KEY(source_id, generation, edition_id, work_id, position),
    FOREIGN KEY(source_id, generation, edition_id, work_id)
        REFERENCES catalog_publication(source_id, generation, edition_id, work_id)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS catalog_publication_popularity (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    want_to_read_count INTEGER NOT NULL,
    currently_reading_count INTEGER NOT NULL,
    already_read_count INTEGER NOT NULL,
    reading_log_count INTEGER NOT NULL,
    rating_count INTEGER NOT NULL,
    rating_sum INTEGER NOT NULL,
    PRIMARY KEY(source_id, generation, edition_id, work_id),
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
CREATE INDEX IF NOT EXISTS catalog_publication_inventory
    ON catalog_publication(source_id, generation, book_format, importable);
CREATE INDEX IF NOT EXISTS catalog_publication_acquisition
    ON catalog_publication(source_id, generation, acquisition_url);
CREATE INDEX IF NOT EXISTS catalog_publication_subject_path
    ON catalog_publication_subject_assignment(system_id, subject_path, source_id, generation, edition_id, work_id);
CREATE INDEX IF NOT EXISTS catalog_publication_toc_title
    ON catalog_publication_toc(source_id, generation, normalized_title, edition_id, work_id);
CREATE INDEX IF NOT EXISTS catalog_epub_extraction_status
    ON catalog_epub_extraction(source_id, status);
CREATE INDEX IF NOT EXISTS catalog_epub_failure_category
    ON catalog_epub_failure(category, retryable, last_failed_at);
CREATE TABLE IF NOT EXISTS catalog_page_link (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    from_locator TEXT NOT NULL,
    to_locator TEXT NOT NULL,
    PRIMARY KEY(source_id, generation, from_locator, to_locator),
    FOREIGN KEY(source_id, generation, from_locator)
        REFERENCES catalog_page(source_id, generation, locator)
        ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS catalog_page_link_target
    ON catalog_page_link(source_id, generation, to_locator);
"#;

#[derive(Debug)]
pub struct CatalogCacheError(String);

impl CatalogCacheError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn is_database_locked(&self) -> bool {
        let message = self.0.to_ascii_lowercase();
        message.contains("database is locked") || message.contains("database table is locked")
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

/// The same lossless EPUB package metadata retained for imported library
/// books, plus the publication's hierarchical navigation document. This is a
/// catalogue-server sidecar and is deliberately not repeated in every card.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogEpubMetadata {
    pub book: library_model::BookMetadataState,
    pub toc: Vec<library_model::PublicationTocEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogSearchMatches {
    pub title_matches: Vec<SequencedCatalogPublication>,
    pub author_matches: Vec<SequencedCatalogPublication>,
    pub toc_matches: Vec<SequencedCatalogPublication>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSourceState {
    pub epoch: String,
    pub provider_epoch: Option<String>,
    pub active_generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSourceInventory {
    pub source_id: CatalogSourceId,
    pub crawl_status: String,
    pub last_crawl_started_at: Option<u64>,
    pub last_crawl_succeeded_at: Option<u64>,
    pub last_crawl_error: Option<String>,
    pub crawl_frontier_count: u64,
    pub active_page_count: u64,
    pub active_publication_count: u64,
    pub active_epub_count: u64,
    pub epub_extractions: CatalogEpubExtractionInventory,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogInventory {
    pub active_page_count: u64,
    pub active_publication_count: u64,
    pub active_epub_count: u64,
    pub epub_extractions: CatalogEpubExtractionInventory,
    pub metadata_backfill: CatalogMetadataBackfillInventory,
    pub sources: Vec<CatalogSourceInventory>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogMetadataBackfillInventory {
    pub pending: u64,
    pub running: u64,
    pub succeeded: u64,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogEpubExtractionInventory {
    pub pending: u64,
    pub running: u64,
    pub downloading: u64,
    pub extracting: u64,
    pub enriching: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub attempts: u64,
    pub downloaded_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogEpubExtractionJob {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub acquisition_url: String,
    pub book_format: CatalogBookFormat,
    pub cover_key: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogMetadataBackfillJob {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub acquisition_url: String,
    pub metadata: CatalogEpubMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogMetadataBackfillUpdate {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub acquisition_url: String,
    pub subjects: Vec<library_model::PublicationSubject>,
    pub popularity: Option<CatalogPublicationPopularity>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogEpubFailureExample {
    pub source_id: CatalogSourceId,
    pub acquisition_host: String,
    pub publication_id: String,
    pub attempts: u64,
    pub last_failed_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogEpubFailureDiagnostic {
    pub category: String,
    pub retryable: bool,
    pub count: u64,
    pub first_failed_at: u64,
    pub last_failed_at: u64,
    pub next_retry_at: Option<u64>,
    pub examples: Vec<CatalogEpubFailureExample>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogPublicationPopularity {
    pub want_to_read_count: u64,
    pub currently_reading_count: u64,
    pub already_read_count: u64,
    pub rating_count: u64,
    pub rating_sum: u64,
}

impl CatalogPublicationPopularity {
    pub fn reading_log_count(&self) -> u64 {
        self.want_to_read_count.saturating_add(self.currently_reading_count).saturating_add(self.already_read_count)
    }
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
        {
            let connection = Connection::open(&path)?;
            connection.busy_timeout(Duration::from_secs(10))?;
            connection.pragma_update(None, "journal_mode", "WAL")?;
        }
        let manager = SqliteConnectionManager::file(&path).with_init(configure_connection);
        let pool = Pool::builder().max_size(8).build(manager)?;
        {
            let connection = pool.get()?;
            connection.execute_batch(CREATE_SCHEMA)?;
            connection.pragma_update(None, "user_version", CATALOG_CACHE_FILE_VERSION)?;
            let now = unix_timestamp()?;
            connection.execute(
                "INSERT OR IGNORE INTO catalog_epub_failure(source_id,acquisition_url,category,retryable,first_failed_at,last_failed_at,next_retry_at)
                 SELECT source_id,acquisition_url,'legacy_unclassified',1,?1,?1,?2
                 FROM catalog_epub_extraction WHERE status='failed'",
                params![now, now.saturating_add(15 * 60)],
            )?;
            connection.execute("UPDATE catalog_source SET crawl_status='failed', last_crawl_error='crawl interrupted by service restart' WHERE crawl_status='running'", [])?;
            connection.execute("UPDATE catalog_epub_extraction SET status='pending', phase=NULL, last_error='extraction interrupted by service restart' WHERE status='running'", [])?;
            connection.execute("UPDATE catalog_metadata_backfill SET status='pending', last_error='metadata backfill interrupted by service restart' WHERE status='running'", [])?;
        }
        Ok(Self { pool })
    }

    /// Rewrites cached publication payloads and page snapshots for one source.
    /// The publication sequence is intentionally preserved because this is a
    /// local presentation-data normalization, not a provider-side revision.
    pub fn rewrite_source_publications(&self, source_id: &CatalogSourceId, mut rewrite: impl FnMut(&mut CatalogPublication) -> bool) -> Result<usize, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let publications = {
            let mut statement = transaction.prepare("SELECT generation, edition_id, work_id, payload FROM catalog_publication WHERE source_id=?1")?;
            let rows = statement.query_map([source_id.as_str()], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Vec<u8>>(3)?)))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut changed = 0;
        for (generation, edition_id, work_id, payload) in publications {
            let mut publication: CatalogPublication = decode_cached(&payload)?;
            if rewrite(&mut publication) {
                let payload = wire::encode(&publication)?;
                let payload_hash = blake3::hash(&payload).to_hex().to_string();
                transaction.execute(
                    "UPDATE catalog_publication SET payload=?5, payload_hash=?6 WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
                    params![source_id.as_str(), generation, edition_id, work_id, payload, payload_hash],
                )?;
                changed += 1;
            }
        }

        let pages = {
            let mut statement = transaction.prepare("SELECT generation, locator, payload FROM catalog_page WHERE source_id=?1")?;
            let rows = statement.query_map([source_id.as_str()], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Vec<u8>>(2)?)))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for (generation, locator, payload) in pages {
            let mut page = decode_page(&payload)?;
            let page_changed = page.publications.iter_mut().chain(page.groups.iter_mut().flat_map(|group| group.publications.iter_mut())).fold(false, |changed, publication| rewrite(publication) || changed);
            if page_changed {
                transaction.execute("UPDATE catalog_page SET payload=?4 WHERE source_id=?1 AND generation=?2 AND locator=?3", params![source_id.as_str(), generation, locator, wire::encode(&page)?])?;
            }
        }
        transaction.commit()?;
        Ok(changed)
    }

    pub fn inventory(&self) -> Result<CatalogInventory, CatalogCacheError> {
        let connection = self.pool.get()?;
        let mut statement = connection.prepare(
            "SELECT source_id, crawl_status, last_crawl_started_at, last_crawl_succeeded_at, last_crawl_error,
                    active_page_count, active_publication_count, active_epub_count,
                    (CASE WHEN NOT EXISTS(
                         SELECT 1 FROM catalog_page root
                         WHERE root.source_id=s.source_id AND root.generation=s.active_generation AND root.locator=s.source_url
                     ) THEN 1 ELSE 0 END) + (
                         SELECT COUNT(DISTINCT link.to_locator)
                         FROM catalog_page_link link
                         LEFT JOIN catalog_page page
                           ON page.source_id=link.source_id AND page.generation=link.generation AND page.locator=link.to_locator
                         WHERE link.source_id=s.source_id AND link.generation=s.active_generation AND page.locator IS NULL
                     ),
                    (SELECT COUNT(*) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id AND e.status='pending'),
                    (SELECT COUNT(*) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id AND e.status='running'),
                    (SELECT COUNT(*) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id AND e.status='running' AND e.phase='downloading'),
                    (SELECT COUNT(*) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id AND e.status='running' AND e.phase='extracting'),
                    (SELECT COUNT(*) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id AND e.status='running' AND e.phase='enriching'),
                    (SELECT COUNT(*) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id AND e.status='succeeded'),
                    (SELECT COUNT(*) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id AND e.status='failed'),
                    (SELECT COALESCE(SUM(attempts), 0) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id),
                    (SELECT COALESCE(SUM(downloaded_bytes), 0) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id)
             FROM catalog_source s ORDER BY source_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(CatalogSourceInventory {
                source_id: CatalogSourceId::parse(&row.get::<_, String>(0)?).map_err(|error| sql_decode_error(std::io::Error::new(std::io::ErrorKind::InvalidData, error)))?,
                crawl_status: row.get(1)?,
                last_crawl_started_at: optional_stored_u64(row.get(2)?, "last crawl started at")?,
                last_crawl_succeeded_at: optional_stored_u64(row.get(3)?, "last crawl succeeded at")?,
                last_crawl_error: row.get(4)?,
                active_page_count: stored_u64(row.get(5)?, "active page count")?,
                active_publication_count: stored_u64(row.get(6)?, "active publication count")?,
                active_epub_count: stored_u64(row.get(7)?, "active EPUB count")?,
                crawl_frontier_count: stored_u64(row.get(8)?, "crawl frontier count")?,
                epub_extractions: CatalogEpubExtractionInventory {
                    pending: stored_u64(row.get(9)?, "pending EPUB extraction count")?,
                    running: stored_u64(row.get(10)?, "running EPUB extraction count")?,
                    downloading: stored_u64(row.get(11)?, "downloading publication count")?,
                    extracting: stored_u64(row.get(12)?, "extracting publication count")?,
                    enriching: stored_u64(row.get(13)?, "enriching publication count")?,
                    succeeded: stored_u64(row.get(14)?, "succeeded EPUB extraction count")?,
                    failed: stored_u64(row.get(15)?, "failed EPUB extraction count")?,
                    attempts: stored_u64(row.get(16)?, "EPUB extraction attempt count")?,
                    downloaded_bytes: stored_u64(row.get(17)?, "EPUB extraction downloaded bytes")?,
                },
            })
        })?;
        let sources = rows.collect::<Result<Vec<_>, _>>()?;
        let metadata_backfill = CatalogMetadataBackfillInventory {
            pending: connection.query_row("SELECT COUNT(*) FROM catalog_metadata_backfill WHERE status='pending'", [], |row| stored_u64(row.get(0)?, "pending metadata backfill count"))?,
            running: connection.query_row("SELECT COUNT(*) FROM catalog_metadata_backfill WHERE status='running'", [], |row| stored_u64(row.get(0)?, "running metadata backfill count"))?,
            succeeded: connection.query_row("SELECT COUNT(*) FROM catalog_metadata_backfill WHERE status='succeeded'", [], |row| stored_u64(row.get(0)?, "succeeded metadata backfill count"))?,
            last_error: connection.query_row("SELECT last_error FROM catalog_metadata_backfill WHERE last_error IS NOT NULL ORDER BY last_attempt_at DESC LIMIT 1", [], |row| row.get(0)).optional()?,
        };
        Ok(CatalogInventory {
            active_page_count: sources.iter().map(|source| source.active_page_count).sum(),
            active_publication_count: sources.iter().map(|source| source.active_publication_count).sum(),
            active_epub_count: sources.iter().map(|source| source.active_epub_count).sum(),
            epub_extractions: sources.iter().fold(CatalogEpubExtractionInventory::default(), |mut total, source| {
                total.pending += source.epub_extractions.pending;
                total.running += source.epub_extractions.running;
                total.downloading += source.epub_extractions.downloading;
                total.extracting += source.epub_extractions.extracting;
                total.enriching += source.epub_extractions.enriching;
                total.succeeded += source.epub_extractions.succeeded;
                total.failed += source.epub_extractions.failed;
                total.attempts += source.epub_extractions.attempts;
                total.downloaded_bytes += source.epub_extractions.downloaded_bytes;
                total
            }),
            metadata_backfill,
            sources,
        })
    }

    /// Returns bounded, normalized diagnostics only. Raw acquisition URLs and
    /// raw upstream errors deliberately remain inside the protected cache.
    pub fn epub_failure_diagnostics(&self) -> Result<Vec<CatalogEpubFailureDiagnostic>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let mut statement = connection.prepare(
            "SELECT failure.category,failure.retryable,failure.first_failed_at,failure.last_failed_at,failure.next_retry_at,
                    extraction.source_id,extraction.acquisition_url,COALESCE(extraction.edition_id,''),extraction.attempts
             FROM catalog_epub_failure failure
             JOIN catalog_epub_extraction extraction
               ON extraction.source_id=failure.source_id AND extraction.acquisition_url=failure.acquisition_url
             WHERE extraction.status='failed'
             ORDER BY failure.category,failure.retryable,failure.last_failed_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, bool>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
            ))
        })?;
        let mut groups = BTreeMap::<(String, bool), CatalogEpubFailureDiagnostic>::new();
        for row in rows {
            let (category, retryable, first_failed_at, last_failed_at, next_retry_at, source_id, acquisition_url, publication_id, attempts) = row?;
            let key = (category.clone(), retryable);
            let entry = groups.entry(key).or_insert_with(|| CatalogEpubFailureDiagnostic { category, retryable, count: 0, first_failed_at: u64::MAX, last_failed_at: 0, next_retry_at: None, examples: Vec::new() });
            entry.count = entry.count.saturating_add(1);
            let first_failed_at = stored_u64(first_failed_at, "first extraction failure timestamp")?;
            let last_failed_at = stored_u64(last_failed_at, "last extraction failure timestamp")?;
            entry.first_failed_at = entry.first_failed_at.min(first_failed_at);
            entry.last_failed_at = entry.last_failed_at.max(last_failed_at);
            if let Some(next_retry_at) = next_retry_at {
                let next_retry_at = stored_u64(next_retry_at, "next extraction retry timestamp")?;
                entry.next_retry_at = Some(entry.next_retry_at.map_or(next_retry_at, |current| current.min(next_retry_at)));
            }
            if entry.examples.len() < 3 {
                let source_id = CatalogSourceId::parse(&source_id).map_err(CatalogCacheError::new)?;
                let acquisition_host = url::Url::parse(&acquisition_url).ok().and_then(|url| url.host_str().map(str::to_owned)).unwrap_or_else(|| "invalid-host".to_owned());
                entry.examples.push(CatalogEpubFailureExample { source_id, acquisition_host, publication_id, attempts: stored_u64(attempts, "extraction attempt count")?, last_failed_at });
            }
        }
        let mut diagnostics = groups.into_values().collect::<Vec<_>>();
        diagnostics.sort_by(|left, right| right.count.cmp(&left.count).then_with(|| left.category.cmp(&right.category)));
        Ok(diagnostics)
    }

    #[cfg(test)]
    pub fn successful_epub_extractions(&self, source_id: &CatalogSourceId) -> Result<HashSet<String>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let mut statement = connection.prepare("SELECT acquisition_url FROM catalog_epub_extraction WHERE source_id=?1 AND status='succeeded' AND result_payload IS NOT NULL AND enrichment_version=?2")?;
        let rows = statement.query_map(params![source_id.as_str(), CATALOG_EPUB_ENRICHMENT_VERSION], |row| row.get(0))?;
        rows.collect::<Result<HashSet<_>, _>>().map_err(Into::into)
    }

    #[cfg(test)]
    pub fn pending_epub_extractions(&self, source_id: &CatalogSourceId) -> Result<HashSet<String>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let mut statement = connection.prepare("SELECT acquisition_url FROM catalog_epub_extraction WHERE source_id=?1 AND status='pending'")?;
        let rows = statement.query_map([source_id.as_str()], |row| row.get(0))?;
        rows.collect::<Result<HashSet<_>, _>>().map_err(Into::into)
    }

    pub fn epub_metadata(&self, source_id: &CatalogSourceId, acquisition_url: &str) -> Result<Option<CatalogEpubMetadata>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let payload = connection
            .query_row("SELECT metadata_payload FROM catalog_epub_extraction WHERE source_id=?1 AND acquisition_url=?2 AND status='succeeded'", params![source_id.as_str(), acquisition_url], |row| row.get::<_, Option<Vec<u8>>>(0))
            .optional()?
            .flatten();
        payload.map(|payload| decode_cached(&payload)).transpose().map_err(Into::into)
    }

    pub fn schedule_metadata_backfill(&self) -> Result<usize, CatalogCacheError> {
        let connection = self.pool.get()?;
        connection
            .execute(
                "INSERT INTO catalog_metadata_backfill(source_id,acquisition_url,version,status)
                 SELECT extraction.source_id,extraction.acquisition_url,?1,'pending'
                 FROM catalog_epub_extraction extraction
                 JOIN catalog_source source ON source.source_id=extraction.source_id
                 JOIN catalog_publication publication
                   ON publication.source_id=extraction.source_id AND publication.generation=source.active_generation
                  AND publication.acquisition_url=extraction.acquisition_url
                 WHERE extraction.status='succeeded' AND extraction.metadata_payload IS NOT NULL
                   AND NOT EXISTS(
                       SELECT 1 FROM catalog_metadata_backfill current
                       WHERE current.source_id=extraction.source_id AND current.acquisition_url=extraction.acquisition_url AND current.version=?1
                   )
                 LIMIT 1000
                 ON CONFLICT(source_id,acquisition_url) DO UPDATE SET
                    version=excluded.version,status='pending',last_error=NULL
                 WHERE catalog_metadata_backfill.version!=excluded.version",
                [CATALOG_METADATA_BACKFILL_VERSION],
            )
            .map_err(Into::into)
    }

    pub fn claim_metadata_backfill_batch(&self, limit: usize) -> Result<Vec<CatalogMetadataBackfillJob>, CatalogCacheError> {
        let limit = limit.clamp(1, 256);
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let jobs = {
            let mut statement = transaction.prepare(
                "SELECT backfill.source_id,backfill.acquisition_url,extraction.edition_id,extraction.work_id,extraction.metadata_payload
                 FROM catalog_metadata_backfill backfill
                 JOIN catalog_epub_extraction extraction
                   ON extraction.source_id=backfill.source_id AND extraction.acquisition_url=backfill.acquisition_url
                 JOIN catalog_source source ON source.source_id=backfill.source_id
                 JOIN catalog_publication publication
                   ON publication.source_id=backfill.source_id AND publication.generation=source.active_generation
                  AND publication.acquisition_url=backfill.acquisition_url
                 WHERE backfill.status='pending' AND backfill.version=?1
                   AND extraction.status='succeeded' AND extraction.metadata_payload IS NOT NULL
                 ORDER BY backfill.attempts,backfill.source_id,backfill.acquisition_url LIMIT ?2",
            )?;
            let rows = statement.query_map(params![CATALOG_METADATA_BACKFILL_VERSION, storage_i64(limit as u64, "metadata backfill batch size")?], |row| {
                let source = row.get::<_, String>(0)?;
                let edition_id = row.get(2)?;
                let work_id = row.get::<_, Option<String>>(3)?.filter(|value| !value.is_empty());
                let payload = row.get::<_, Vec<u8>>(4)?;
                let metadata = decode_cached(&payload).map_err(sql_decode_error)?;
                let source_id = CatalogSourceId::parse(&source).map_err(|error| sql_decode_error(std::io::Error::new(std::io::ErrorKind::InvalidData, error)))?;
                Ok(CatalogMetadataBackfillJob { source_id, publication: CatalogPublicationIdentity { edition_id, work_id }, acquisition_url: row.get(1)?, metadata })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let now = unix_timestamp()?;
        for job in &jobs {
            transaction.execute(
                "UPDATE catalog_metadata_backfill SET status='running',attempts=attempts+1,last_attempt_at=?3,last_error=NULL
                 WHERE source_id=?1 AND acquisition_url=?2 AND status='pending'",
                params![job.source_id.as_str(), job.acquisition_url, now],
            )?;
        }
        transaction.commit()?;
        Ok(jobs)
    }

    pub fn finish_metadata_backfill_batch(&self, updates: &[CatalogMetadataBackfillUpdate]) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for update in updates {
            let generation = active_generation_in(&transaction, &update.source_id)?;
            if sequence_in(&transaction, &update.source_id, generation, &update.publication)?.is_some() {
                replace_publication_subjects(&transaction, &update.source_id, generation, &update.publication, &update.subjects)?;
                if let Some(popularity) = &update.popularity {
                    let work_id = update.publication.work_id.as_deref().unwrap_or("");
                    transaction.execute(
                        "INSERT OR REPLACE INTO catalog_publication_popularity(
                            source_id,generation,edition_id,work_id,want_to_read_count,currently_reading_count,already_read_count,reading_log_count,rating_count,rating_sum
                         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                        params![
                            update.source_id.as_str(),
                            storage_i64(generation, "generation")?,
                            update.publication.edition_id,
                            work_id,
                            storage_i64(popularity.want_to_read_count, "want-to-read count")?,
                            storage_i64(popularity.currently_reading_count, "currently-reading count")?,
                            storage_i64(popularity.already_read_count, "already-read count")?,
                            storage_i64(popularity.reading_log_count(), "reading-log count")?,
                            storage_i64(popularity.rating_count, "rating count")?,
                            storage_i64(popularity.rating_sum, "rating sum")?
                        ],
                    )?;
                }
            }
            transaction.execute("UPDATE catalog_metadata_backfill SET status='succeeded',last_error=NULL WHERE source_id=?1 AND acquisition_url=?2 AND status='running'", params![update.source_id.as_str(), update.acquisition_url])?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn release_metadata_backfill_batch(&self, jobs: &[CatalogMetadataBackfillJob], error: &str) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let error = bounded_crawl_error(error);
        for job in jobs {
            transaction.execute("UPDATE catalog_metadata_backfill SET status='pending',last_error=?3 WHERE source_id=?1 AND acquisition_url=?2 AND status='running'", params![job.source_id.as_str(), job.acquisition_url, error])?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn queue_crawls(&self, source_ids: &[CatalogSourceId]) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for source_id in source_ids {
            transaction.execute("UPDATE catalog_source SET crawl_status='queued', last_crawl_error=NULL WHERE source_id=?1 AND crawl_status!='running'", [source_id.as_str()])?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn queue_epub_extractions(&self, source_id: &CatalogSourceId, acquisition_urls: &[String], force: bool) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for acquisition_url in acquisition_urls {
            transaction.execute(
                "INSERT INTO catalog_epub_extraction(source_id, acquisition_url, status) VALUES(?1, ?2, 'pending')
                 ON CONFLICT(source_id, acquisition_url) DO UPDATE SET
                    status=CASE WHEN ?3 OR catalog_epub_extraction.enrichment_version!=?4 OR (catalog_epub_extraction.status='succeeded' AND catalog_epub_extraction.result_payload IS NULL) THEN 'pending' ELSE catalog_epub_extraction.status END,
                    phase=CASE WHEN ?3 OR catalog_epub_extraction.enrichment_version!=?4 OR catalog_epub_extraction.result_payload IS NULL THEN NULL ELSE catalog_epub_extraction.phase END,
                    last_error=CASE WHEN ?3 OR catalog_epub_extraction.enrichment_version!=?4 OR catalog_epub_extraction.result_payload IS NULL THEN NULL ELSE catalog_epub_extraction.last_error END",
                params![source_id.as_str(), acquisition_url, force, CATALOG_EPUB_ENRICHMENT_VERSION],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn retry_failed_epub_extractions(&self, source_id: &CatalogSourceId) -> Result<(), CatalogCacheError> {
        let connection = self.pool.get()?;
        connection.execute(
            "UPDATE catalog_epub_extraction AS extraction SET status='pending',phase=NULL
             WHERE extraction.source_id=?1 AND extraction.status='failed'
               AND EXISTS(
                   SELECT 1 FROM catalog_epub_failure failure
                   WHERE failure.source_id=extraction.source_id AND failure.acquisition_url=extraction.acquisition_url
                     AND failure.retryable=1 AND COALESCE(failure.next_retry_at,0)<=?2
               )",
            params![source_id.as_str(), unix_timestamp()?],
        )?;
        Ok(())
    }

    /// Atomically claims one bounded job. Rows created by older binaries do
    /// not contain job metadata and are ignored until queue reconciliation
    /// backfills them from the active publication table.
    pub fn claim_next_epub_extraction(&self, source_id: &CatalogSourceId) -> Result<Option<CatalogEpubExtractionJob>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let claimed = connection
            .query_row(
                "UPDATE catalog_epub_extraction
                 SET status='running', phase='downloading', attempts=attempts+1, last_attempt_at=?2, last_error=NULL
                 WHERE (source_id, acquisition_url)=(
                     SELECT source_id, acquisition_url FROM catalog_epub_extraction
                     WHERE source_id=?1 AND status='pending'
                       AND edition_id IS NOT NULL AND book_format IS NOT NULL
                       AND EXISTS(
                           SELECT 1 FROM catalog_publication publication
                           WHERE publication.source_id=catalog_epub_extraction.source_id
                             AND publication.generation=(SELECT active_generation FROM catalog_source WHERE source_id=?1)
                             AND publication.acquisition_url=catalog_epub_extraction.acquisition_url
                       )
                     ORDER BY attempts, acquisition_url LIMIT 1
                 ) AND status='pending'
                 RETURNING source_id, edition_id, work_id, acquisition_url, book_format, cover_key",
                params![source_id.as_str(), unix_timestamp()?],
                |row| {
                    let source_id = CatalogSourceId::parse(&row.get::<_, String>(0)?).map_err(|error| sql_decode_error(std::io::Error::new(std::io::ErrorKind::InvalidData, error)))?;
                    let edition_id = row.get(1)?;
                    let work_id = row.get::<_, Option<String>>(2)?.filter(|value| !value.is_empty());
                    let acquisition_url = row.get(3)?;
                    let format = row.get::<_, String>(4)?;
                    let book_format = catalog_book_format_from_storage(&format).ok_or_else(|| sql_decode_error(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("unsupported extraction book format {format}"))))?;
                    Ok(CatalogEpubExtractionJob { source_id, publication: CatalogPublicationIdentity { edition_id, work_id }, acquisition_url, book_format, cover_key: row.get(5)? })
                },
            )
            .optional()?;
        Ok(claimed)
    }

    pub fn begin_epub_extraction(&self, source_id: &CatalogSourceId, acquisition_url: &str) -> Result<bool, CatalogCacheError> {
        let connection = self.pool.get()?;
        let changed = connection.execute(
            "UPDATE catalog_epub_extraction SET status='running', phase='downloading', attempts=attempts+1, last_attempt_at=?3, last_error=NULL WHERE source_id=?1 AND acquisition_url=?2 AND status IN ('pending','failed')",
            params![source_id.as_str(), acquisition_url, unix_timestamp()?],
        )?;
        Ok(changed == 1)
    }

    pub fn epub_extraction_status(&self, source_id: &CatalogSourceId, acquisition_url: &str) -> Result<Option<String>, CatalogCacheError> {
        let connection = self.pool.get()?;
        connection.query_row("SELECT status FROM catalog_epub_extraction WHERE source_id=?1 AND acquisition_url=?2", params![source_id.as_str(), acquisition_url], |row| row.get(0)).optional().map_err(Into::into)
    }

    pub fn set_epub_extraction_phase(&self, source_id: &CatalogSourceId, acquisition_url: &str, phase: &str) -> Result<(), CatalogCacheError> {
        if !matches!(phase, "downloading" | "extracting" | "enriching") {
            return Err(CatalogCacheError::new("invalid EPUB extraction phase"));
        }
        let connection = self.pool.get()?;
        connection.execute("UPDATE catalog_epub_extraction SET phase=?3 WHERE source_id=?1 AND acquisition_url=?2 AND status='running'", params![source_id.as_str(), acquisition_url, phase])?;
        Ok(())
    }

    pub fn finish_epub_extraction(&self, source_id: &CatalogSourceId, acquisition_url: &str, downloaded_bytes: u64, publication: &CatalogPublication, metadata: Option<&CatalogEpubMetadata>) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "UPDATE catalog_epub_extraction SET status='succeeded', phase=NULL, downloaded_bytes=?3, last_succeeded_at=?4, last_error=NULL, result_payload=?5, metadata_payload=?6, enrichment_version=?7 WHERE source_id=?1 AND acquisition_url=?2",
            params![
                source_id.as_str(),
                acquisition_url,
                storage_i64(downloaded_bytes, "EPUB extraction downloaded bytes")?,
                unix_timestamp()?,
                wire::encode(publication)?,
                metadata.map(wire::encode).transpose()?,
                CATALOG_EPUB_ENRICHMENT_VERSION
            ],
        )?;
        transaction.execute("DELETE FROM catalog_epub_failure WHERE source_id=?1 AND acquisition_url=?2", params![source_id.as_str(), acquisition_url])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn active_publication(&self, source_id: &CatalogSourceId, identity: &CatalogPublicationIdentity) -> Result<Option<CatalogPublication>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(None) };
        sequenced_publication_in(&connection, source_id, generation, identity).map(|publication| publication.map(|publication| publication.publication))
    }

    pub fn enrich_active_publication(
        &self, source_id: &CatalogSourceId, publication: &CatalogPublication, subjects: &[library_model::PublicationSubject], toc: &[library_model::PublicationTocEntry], popularity: Option<&CatalogPublicationPopularity>,
    ) -> Result<bool, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = active_generation_in(&transaction, source_id)?;
        let (edition_id, work_id) = publication_key(publication);
        let exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4)",
            params![source_id.as_str(), storage_i64(generation, "generation")?, edition_id, work_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            return Ok(false);
        }
        upsert_active_publication(&transaction, source_id, generation, publication)?;
        // Completion can make a cached cover available without changing the
        // publication payload. Always advance the sequence so connected
        // clients refresh that card and retry its cover request.
        let completion_sequence = allocate_sequence(&transaction, source_id)?;
        transaction.execute(
            "UPDATE catalog_publication SET sequence=?5 WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, edition_id, work_id, completion_sequence],
        )?;
        replace_publication_subjects(&transaction, source_id, generation, &publication.identity, subjects)?;
        replace_publication_toc(&transaction, source_id, generation, &publication.identity, toc)?;
        if let Some(popularity) = popularity {
            transaction.execute(
                "INSERT OR REPLACE INTO catalog_publication_popularity(
                    source_id,generation,edition_id,work_id,want_to_read_count,currently_reading_count,already_read_count,reading_log_count,rating_count,rating_sum
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    source_id.as_str(),
                    storage_i64(generation, "generation")?,
                    edition_id,
                    work_id,
                    storage_i64(popularity.want_to_read_count, "want-to-read count")?,
                    storage_i64(popularity.currently_reading_count, "currently-reading count")?,
                    storage_i64(popularity.already_read_count, "already-read count")?,
                    storage_i64(popularity.reading_log_count(), "reading-log count")?,
                    storage_i64(popularity.rating_count, "rating count")?,
                    storage_i64(popularity.rating_sum, "rating sum")?,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(true)
    }

    /// Removes a definitively unusable cover without replacing the subject
    /// evidence produced by an earlier successful EPUB extraction. The saved
    /// extraction payload is updated too, so a later crawl cannot restore the
    /// rejected URL while overlaying persistent metadata.
    pub fn clear_active_publication_cover(&self, source_id: &CatalogSourceId, identity: &CatalogPublicationIdentity, acquisition_url: &str, expected_image_url: &str) -> Result<bool, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = active_generation_in(&transaction, source_id)?;
        let Some(mut publication) = sequenced_publication_in(&transaction, source_id, generation, identity)?.map(|publication| publication.publication) else { return Ok(false) };
        if publication.image_url.as_deref() != Some(expected_image_url) {
            return Ok(false);
        }
        publication.image_url = None;
        upsert_active_publication(&transaction, source_id, generation, &publication)?;
        let completion_sequence = allocate_sequence(&transaction, source_id)?;
        let (edition_id, work_id) = publication_key(&publication);
        transaction.execute(
            "UPDATE catalog_publication SET sequence=?5 WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, edition_id, work_id, completion_sequence],
        )?;
        transaction.execute("UPDATE catalog_epub_extraction SET result_payload=?3 WHERE source_id=?1 AND acquisition_url=?2", params![source_id.as_str(), acquisition_url, wire::encode(&publication)?])?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn fail_epub_extraction(&self, source_id: &CatalogSourceId, acquisition_url: &str, error: &str, category: &str, retryable: bool) -> Result<(), CatalogCacheError> {
        if category.is_empty() || category.len() > 64 || !category.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_') {
            return Err(CatalogCacheError::new("invalid EPUB failure category"));
        }
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = unix_timestamp()?;
        let attempts = transaction.query_row("SELECT attempts FROM catalog_epub_extraction WHERE source_id=?1 AND acquisition_url=?2", params![source_id.as_str(), acquisition_url], |row| row.get::<_, i64>(0))?;
        let retry_delay = match attempts {
            i64::MIN..=1 => 15 * 60,
            2 => 60 * 60,
            3 => 6 * 60 * 60,
            _ => 24 * 60 * 60,
        };
        let next_retry_at = retryable.then(|| now.saturating_add(retry_delay));
        transaction.execute("UPDATE catalog_epub_extraction SET status='failed', phase=NULL, last_error=?3 WHERE source_id=?1 AND acquisition_url=?2", params![source_id.as_str(), acquisition_url, bounded_crawl_error(error)])?;
        transaction.execute(
            "INSERT INTO catalog_epub_failure(source_id,acquisition_url,category,retryable,first_failed_at,last_failed_at,next_retry_at)
             VALUES(?1,?2,?3,?4,?5,?5,?6)
             ON CONFLICT(source_id,acquisition_url) DO UPDATE SET
                 category=excluded.category,retryable=excluded.retryable,last_failed_at=excluded.last_failed_at,next_retry_at=excluded.next_retry_at",
            params![source_id.as_str(), acquisition_url, category, retryable, now, next_retry_at],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn retain_epub_extractions(&self, source_id: &CatalogSourceId, retained_urls: &HashSet<String>) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = {
            let mut statement = transaction.prepare("SELECT acquisition_url FROM catalog_epub_extraction WHERE source_id=?1")?;
            let rows = statement.query_map([source_id.as_str()], |row| row.get(0))?;
            rows.collect::<Result<Vec<String>, _>>()?
        };
        for acquisition_url in existing {
            if !retained_urls.contains(&acquisition_url) {
                transaction.execute("DELETE FROM catalog_epub_extraction WHERE source_id=?1 AND acquisition_url=?2", params![source_id.as_str(), acquisition_url])?;
            }
        }
        transaction.commit()?;
        Ok(())
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
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let state = source_state_in(&transaction, source_id)?.expect("source inserted");
        transaction.commit()?;
        Ok(state)
    }

    pub fn set_provider_epoch(&self, source_id: &CatalogSourceId, source_url: &str, provider_epoch: &str) -> Result<bool, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
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
        // Serialization can be substantial for feeds with embedded images.
        // Do it before BEGIN IMMEDIATE acquires SQLite's sole writer lock.
        let page_payload = wire::encode(page)?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let generation = active_generation_in(&transaction, source_id)?;
        put_page_row(&transaction, source_id, generation, page, &page_payload, etag, last_modified)?;
        for publication in unique_page_publications(page) {
            upsert_active_publication(&transaction, source_id, generation, publication)?;
            queue_publication_extraction(&transaction, source_id, generation, publication, false)?;
        }
        refresh_active_counts(&transaction, source_id, generation)?;
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

    pub fn has_pages(&self, source_id: &CatalogSourceId) -> Result<bool, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(false) };
        connection.query_row("SELECT EXISTS(SELECT 1 FROM catalog_page WHERE source_id=?1 AND generation=?2)", params![source_id.as_str(), storage_i64(generation, "generation")?], |row| row.get(0)).map_err(Into::into)
    }

    /// Checks for unfinished graph discovery without materializing every
    /// missing locator. The crawler walks cached pages into its own temporary
    /// disk frontier when this returns true.
    pub fn has_missing_page_frontier(&self, source_id: &CatalogSourceId, root_locator: &str) -> Result<bool, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(true) };
        let generation = storage_i64(generation, "generation")?;
        let root_exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM catalog_page WHERE source_id=?1 AND generation=?2 AND locator=?3)", params![source_id.as_str(), generation, root_locator], |row| row.get::<_, bool>(0))?;
        if !root_exists {
            return Ok(true);
        }
        connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM catalog_page_link link
                     LEFT JOIN catalog_page page
                       ON page.source_id=link.source_id AND page.generation=link.generation AND page.locator=link.to_locator
                     WHERE link.source_id=?1 AND link.generation=?2 AND page.locator IS NULL
                 )",
                params![source_id.as_str(), generation],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Returns only links whose target page is absent from the active
    /// generation. Page payloads remain in SQLite.
    pub fn missing_page_frontier(&self, source_id: &CatalogSourceId, root_locator: &str) -> Result<Vec<String>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(vec![root_locator.to_owned()]) };
        let generation = storage_i64(generation, "generation")?;
        let mut frontier = Vec::new();
        let root_exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM catalog_page WHERE source_id=?1 AND generation=?2 AND locator=?3)", params![source_id.as_str(), generation, root_locator], |row| row.get::<_, bool>(0))?;
        if !root_exists {
            frontier.push(root_locator.to_owned());
        }
        let mut statement = connection.prepare(
            "SELECT DISTINCT link.to_locator
             FROM catalog_page_link link
             LEFT JOIN catalog_page page
               ON page.source_id=link.source_id AND page.generation=link.generation AND page.locator=link.to_locator
             WHERE link.source_id=?1 AND link.generation=?2 AND page.locator IS NULL
             ORDER BY link.to_locator",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), generation], |row| row.get::<_, String>(0))?;
        frontier.extend(rows.collect::<Result<Vec<_>, _>>()?);
        Ok(frontier)
    }

    pub fn enrich_page_from_epub_extractions(&self, source_id: &CatalogSourceId, page: &mut CatalogPage) -> Result<(), CatalogCacheError> {
        let connection = self.pool.get()?;
        let mut statement = connection.prepare(
            "SELECT result_payload FROM catalog_epub_extraction
             WHERE source_id=?1 AND acquisition_url=?2 AND status='succeeded' AND result_payload IS NOT NULL",
        )?;
        for publication in page.publications.iter_mut().chain(page.groups.iter_mut().flat_map(|group| group.publications.iter_mut())) {
            let Some(extraction) = publication_extraction(publication) else { continue };
            let payload = statement.query_row(params![source_id.as_str(), extraction.acquisition_url], |row| row.get::<_, Vec<u8>>(0)).optional()?;
            let Some(payload) = payload else { continue };
            let enriched: CatalogPublication = decode_cached(&payload)?;
            publication.title = enriched.title;
            publication.authors = enriched.authors;
            publication.languages = enriched.languages;
            publication.description = enriched.description;
            publication.image_url = enriched.image_url;
        }
        Ok(())
    }

    pub fn active_publication_for_cover(&self, source_id: &CatalogSourceId, cover_url: &str) -> Result<Option<CatalogPublication>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(None) };
        let payload = connection
            .query_row(
                "SELECT payload FROM catalog_publication
                 WHERE source_id=?1 AND generation=?2 AND cover_url=?3
                 ORDER BY edition_id,work_id LIMIT 1",
                params![source_id.as_str(), storage_i64(generation, "generation")?, cover_url],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?;
        payload.map(|payload| decode_cached(&payload)).transpose().map_err(Into::into)
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
        let Some(mut page) = page_in(&connection, source_id, state.active_generation, locator)? else {
            return Ok(None);
        };
        let known = if request_epoch == Some(state.epoch.as_str()) { known.iter().map(|item| (item.identity.clone(), item.sequence)).collect::<HashMap<_, _>>() } else { HashMap::new() };
        let mut payloads = Vec::new();
        let mut references = HashMap::new();
        let mut popularity = HashMap::new();
        for publication in unique_page_publications(&page.page) {
            let (sequenced, score) =
                sequenced_publication_with_popularity_in(&connection, source_id, state.active_generation, &publication.identity)?.ok_or_else(|| CatalogCacheError::new("catalog page references a missing publication"))?;
            let reference = CatalogPublicationReference { identity: publication.identity.clone(), sequence: sequenced.sequence };
            if known.get(&reference.identity) != Some(&reference.sequence) {
                payloads.push(CatalogPublicationPayload { publication: sequenced.publication, sequence: reference.sequence });
            }
            popularity.insert(reference.identity.clone(), score);
            references.insert(reference.identity.clone(), reference);
        }
        sort_page_publications_by_popularity(&mut page.page, &popularity);
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
        let toc_matches = matches.toc_matches.iter().map(publication_reference).collect::<Vec<_>>();
        let payloads = matches
            .title_matches
            .into_iter()
            .chain(matches.author_matches)
            .chain(matches.toc_matches)
            .filter(|publication| known.get(&publication.publication.identity) != Some(&publication.sequence))
            .map(|publication| CatalogPublicationPayload { publication: publication.publication, sequence: publication.sequence })
            .collect();
        Ok(Some(CatalogSearchResponse { source_id: source_id.clone(), epoch: state.epoch, query: query.to_owned(), title_matches, author_matches, toc_matches, payloads }))
    }

    pub fn apply_search_response(&self, source_url: &str, response: CatalogSearchResponse) -> Result<CatalogSearchMatches, CatalogCacheError> {
        self.set_provider_epoch(&response.source_id, source_url, &response.epoch)?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, &response.source_id, source_url)?;
        let generation = active_generation_in(&transaction, &response.source_id)?;
        for payload in &response.payloads {
            upsert_publication_with_sequence(&transaction, &response.source_id, generation, &payload.publication, payload.sequence)?;
        }
        refresh_active_counts(&transaction, &response.source_id, generation)?;
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
        let toc_matches = response.toc_matches.iter().map(&resolve).collect::<Result<Vec<_>, _>>()?;
        transaction.commit()?;
        Ok(CatalogSearchMatches { title_matches, author_matches, toc_matches })
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
            "SELECT publication.payload, publication.sequence FROM catalog_publication publication
             LEFT JOIN catalog_publication_popularity popularity
               ON popularity.source_id=publication.source_id AND popularity.generation=publication.generation
              AND popularity.edition_id=publication.edition_id AND popularity.work_id=publication.work_id
             WHERE publication.source_id=?1 AND publication.generation=?2 AND instr(publication.normalized_title, ?3)>0
             ORDER BY COALESCE(popularity.reading_log_count,0) DESC, COALESCE(popularity.rating_count,0) DESC,
                      publication.normalized_title, publication.edition_id, publication.work_id",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), generation, normalized_query], decode_sequenced_publication)?;
        for row in rows {
            title_matches.push(row?);
        }

        let mut author_matches = Vec::new();
        let mut statement = connection.prepare(
            "SELECT publication.payload, publication.sequence
             FROM catalog_publication publication
             LEFT JOIN catalog_publication_popularity popularity
               ON popularity.source_id=publication.source_id AND popularity.generation=publication.generation
              AND popularity.edition_id=publication.edition_id AND popularity.work_id=publication.work_id
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
             ORDER BY COALESCE(popularity.reading_log_count,0) DESC, COALESCE(popularity.rating_count,0) DESC,
                      publication.normalized_title, publication.edition_id, publication.work_id",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), generation, normalized_query], decode_sequenced_publication)?;
        for row in rows {
            author_matches.push(row?);
        }
        let mut toc_matches = Vec::new();
        let mut statement = connection.prepare(
            "SELECT publication.payload, publication.sequence
             FROM catalog_publication publication
             LEFT JOIN catalog_publication_popularity popularity
               ON popularity.source_id=publication.source_id AND popularity.generation=publication.generation
              AND popularity.edition_id=publication.edition_id AND popularity.work_id=publication.work_id
             WHERE publication.source_id=?1 AND publication.generation=?2
               AND instr(publication.normalized_title, ?3)=0
               AND NOT EXISTS (
                   SELECT 1 FROM catalog_publication_author author
                   WHERE author.source_id=publication.source_id
                     AND author.generation=publication.generation
                     AND author.edition_id=publication.edition_id
                     AND author.work_id=publication.work_id
                     AND instr(author.normalized_name, ?3)>0
               )
               AND EXISTS (
                   SELECT 1 FROM catalog_publication_toc toc
                   WHERE toc.source_id=publication.source_id
                     AND toc.generation=publication.generation
                     AND toc.edition_id=publication.edition_id
                     AND toc.work_id=publication.work_id
                     AND instr(toc.normalized_title, ?3)>0
               )
             ORDER BY COALESCE(popularity.reading_log_count,0) DESC, COALESCE(popularity.rating_count,0) DESC,
                      publication.normalized_title, publication.edition_id, publication.work_id",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), generation, normalized_query], decode_sequenced_publication)?;
        for row in rows {
            toc_matches.push(row?);
        }
        Ok(CatalogSearchMatches { title_matches, author_matches, toc_matches })
    }

    pub fn begin_crawl(&self, source_id: &CatalogSourceId, source_url: &str) -> Result<CatalogCrawlGeneration, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let active = active_generation_in(&transaction, source_id)?;
        let generation = active.checked_add(1).ok_or_else(|| CatalogCacheError::new("catalog generation overflow"))?;
        transaction.execute("DELETE FROM catalog_page WHERE source_id=?1 AND generation=?2", params![source_id.as_str(), storage_i64(generation, "generation")?])?;
        transaction.execute("DELETE FROM catalog_publication WHERE source_id=?1 AND generation=?2", params![source_id.as_str(), storage_i64(generation, "generation")?])?;
        transaction.execute("UPDATE catalog_source SET last_crawl_started_at=?2, crawl_status='running', last_crawl_error=NULL WHERE source_id=?1", params![source_id.as_str(), unix_timestamp()?])?;
        transaction.commit()?;
        Ok(CatalogCrawlGeneration { source_id: source_id.clone(), generation })
    }

    /// Starts or resumes discovery directly in the active generation. This is
    /// used for catalogues whose graph has never completed, so useful pages do
    /// not remain hidden behind an unbounded atomic crawl.
    pub fn begin_incremental_crawl(&self, source_id: &CatalogSourceId, source_url: &str) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        transaction.execute("UPDATE catalog_source SET last_crawl_started_at=?2, crawl_status='running', last_crawl_error=NULL WHERE source_id=?1", params![source_id.as_str(), unix_timestamp()?])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn finish_incremental_crawl(&self, source_id: &CatalogSourceId, complete: bool) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = active_generation_in(&transaction, source_id)?;
        refresh_active_counts(&transaction, source_id, generation)?;
        if complete {
            transaction.execute("UPDATE catalog_source SET crawl_status='succeeded', last_crawl_succeeded_at=?2, last_crawl_error=NULL WHERE source_id=?1", params![source_id.as_str(), unix_timestamp()?])?;
        } else {
            transaction.execute("UPDATE catalog_source SET crawl_status='partial', last_crawl_error='crawl checkpoint reached; discovery will resume from the persisted frontier' WHERE source_id=?1", [source_id.as_str()])?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn stage_page(&self, crawl: &CatalogCrawlGeneration, page: &CatalogPage, etag: Option<&str>, last_modified: Option<&str>) -> Result<(), CatalogCacheError> {
        let page_payload = wire::encode(page)?;
        let publications = unique_page_publications(page).into_iter().map(PreparedStagedPublication::new).collect::<Result<Vec<_>, _>>()?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let active_generation = active_generation_in(&transaction, &crawl.source_id)?;
        put_page_row(&transaction, &crawl.source_id, crawl.generation, page, &page_payload, etag, last_modified)?;
        for publication in &publications {
            insert_staged_publication(&transaction, &crawl.source_id, crawl.generation, publication)?;
            copy_subject_metadata(&transaction, &crawl.source_id, active_generation, crawl.generation, &publication.publication.identity)?;
            copy_toc_metadata(&transaction, &crawl.source_id, active_generation, crawl.generation, &publication.publication.identity)?;
            copy_popularity_metadata(&transaction, &crawl.source_id, active_generation, crawl.generation, &publication.publication.identity)?;
        }
        transaction.commit()?;
        Ok(())
    }

    /// Replaces raw subject evidence for one staged publication and rebuilds
    /// its deterministic canonical assignments. This deliberately remains a
    /// sidecar to the catalogue wire payload: browsing clients do not need the
    /// potentially large raw metadata in every publication card.
    pub fn stage_publication_subjects(&self, crawl: &CatalogCrawlGeneration, identity: &CatalogPublicationIdentity, subjects: &[library_model::PublicationSubject]) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = storage_i64(crawl.generation, "generation")?;
        let work_id = identity.work_id.as_deref().unwrap_or("");
        let exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4)",
            params![crawl.source_id.as_str(), generation, identity.edition_id, work_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            return Err(CatalogCacheError::new("cannot attach subjects to a publication outside the staged generation"));
        }
        replace_publication_subjects(&transaction, &crawl.source_id, crawl.generation, identity, subjects)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn recommendations(&self, request: &CatalogRecommendationRequest) -> Result<CatalogRecommendationResponse, CatalogCacheError> {
        const MAX_PROFILE_SUBJECTS: usize = 64;
        const MAX_EXCLUSIONS: usize = 10_000;
        const MAX_RESULTS: usize = 100;
        if request.subjects.len() > MAX_PROFILE_SUBJECTS || request.excluded_publications.len() > MAX_EXCLUSIONS {
            return Err(CatalogCacheError::new("catalog recommendation request exceeds its bounded input size"));
        }
        let limit = usize::try_from(request.limit).unwrap_or(MAX_RESULTS).clamp(1, MAX_RESULTS);
        let excluded = request.excluded_publications.iter().map(|item| (item.source_id.as_str().to_owned(), item.publication.edition_id.clone(), item.publication.work_id.clone().unwrap_or_default())).collect::<HashSet<_>>();
        let connection = self.pool.get()?;
        let mut candidates = HashMap::<(String, String, String), (CatalogSourceId, CatalogPublication, u64, HashMap<(String, String), u32>, i64, i64)>::new();
        let mut statement = connection.prepare(
            "SELECT p.source_id,p.edition_id,p.work_id,p.payload,a.subject_path,
                    COALESCE(pop.reading_log_count,0),COALESCE(pop.rating_count,0)
             FROM catalog_publication_subject_assignment a
             JOIN catalog_source s ON s.source_id=a.source_id AND s.active_generation=a.generation
             JOIN catalog_publication p ON p.source_id=a.source_id AND p.generation=a.generation AND p.edition_id=a.edition_id AND p.work_id=a.work_id
             LEFT JOIN catalog_publication_popularity pop
               ON pop.source_id=p.source_id AND pop.generation=p.generation AND pop.edition_id=p.edition_id AND pop.work_id=p.work_id
             WHERE a.system_id=?1 AND p.importable=1
               AND (a.subject_path=?2 OR instr(a.subject_path,?2 || ' / ')=1 OR instr(?2,a.subject_path || ' / ')=1)",
        )?;
        for preference in request.subjects.iter().filter(|item| item.weight > 0 && valid_subject_key(&item.system_id, &item.subject_path)) {
            let rows = statement.query_map(params![preference.system_id.to_ascii_lowercase(), preference.subject_path], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Vec<u8>>(3)?, row.get::<_, String>(4)?, row.get::<_, i64>(5)?, row.get::<_, i64>(6)?))
            })?;
            for row in rows {
                let (source, edition, work, payload, assigned_path, reading_log_count, rating_count) = row?;
                if excluded.contains(&(source.clone(), edition.clone(), work.clone())) {
                    continue;
                }
                let publication: CatalogPublication = decode_cached(&payload)?;
                if !request.supported_languages.is_empty() && !publication.languages.is_empty() && !publication.languages.iter().any(|language| language_supported(language, &request.supported_languages)) {
                    continue;
                }
                let distance = subject_distance(&preference.subject_path, &assigned_path);
                let score = u64::from(preference.weight).saturating_mul(100) / u64::try_from(distance + 1).unwrap_or(u64::MAX);
                let key = (source.clone(), edition, work);
                let source_id = CatalogSourceId::parse(&source).map_err(CatalogCacheError::new)?;
                let entry = candidates.entry(key).or_insert_with(|| (source_id, publication, 0, HashMap::new(), reading_log_count, rating_count));
                entry.2 = entry.2.saturating_add(score);
                entry.3.entry((preference.system_id.to_ascii_lowercase(), assigned_path)).and_modify(|weight| *weight = (*weight).max(preference.weight)).or_insert(preference.weight);
            }
        }
        let mut recommendations = candidates
            .into_values()
            .map(|(source_id, publication, score, matches, reading_log_count, rating_count)| {
                (
                    CatalogRecommendation { source_id, publication, score, matched_subjects: matches.into_iter().map(|((system_id, subject_path), weight)| CatalogSubjectPreference { system_id, subject_path, weight }).collect() },
                    reading_log_count,
                    rating_count,
                )
            })
            .collect::<Vec<_>>();
        recommendations.sort_by(|left, right| {
            right
                .0
                .score
                .cmp(&left.0.score)
                .then_with(|| right.1.cmp(&left.1))
                .then_with(|| right.2.cmp(&left.2))
                .then_with(|| left.0.publication.title.cmp(&right.0.publication.title))
                .then_with(|| left.0.source_id.as_str().cmp(right.0.source_id.as_str()))
        });
        let mut recommendations = recommendations.into_iter().map(|(recommendation, _, _)| recommendation).collect::<Vec<_>>();
        let mut seen_works = HashSet::new();
        recommendations.retain(|recommendation| seen_works.insert(recommendation_dedupe_key(recommendation)));
        recommendations.truncate(limit);
        Ok(CatalogRecommendationResponse { recommendations })
    }

    /// Browses the same canonical subject assignments used by recommendation
    /// matching. The locator is deliberately opaque to clients so taxonomy
    /// storage and hierarchy rules stay here rather than being duplicated in
    /// each UI.
    pub fn browse_subjects(&self, request: &CatalogSubjectBrowseRequest) -> Result<CatalogSubjectBrowseResponse, CatalogCacheError> {
        const MAX_RESULTS: usize = 100;
        const MAX_SOURCE_FILTERS: usize = 256;
        if request.source_ids.len() > MAX_SOURCE_FILTERS {
            return Err(CatalogCacheError::new("too many catalogue source filters"));
        }
        let limit = usize::try_from(request.limit).unwrap_or(MAX_RESULTS).clamp(1, MAX_RESULTS);
        let normalized_query = normalize_search_text(&request.query);
        let location = request.locator.as_deref().map(parse_subject_locator).transpose()?.unwrap_or_default();
        let offset = usize::try_from(location.offset).unwrap_or(usize::MAX);
        let connection = self.pool.get()?;
        if location.system_id.is_none() {
            return browse_subject_roots(&connection, request);
        }

        let mut child_publications = HashMap::<(String, Option<String>, String), HashSet<(String, String, String)>>::new();
        let mut publications = HashMap::<(String, String, String), CatalogSourcedPublication>::new();
        let mut popularity = HashMap::<(String, String, String), (bool, i64, i64, u64, i64)>::new();
        let system_id = location.system_id.as_deref().expect("subject system checked above");
        if !browse_subject_system(system_id) {
            return Err(CatalogCacheError::new("unsupported catalogue subject system"));
        }
        let mut statement = connection.prepare(
            "SELECT a.system_id,a.subject_path,p.source_id,p.edition_id,p.work_id,p.payload,
                    COALESCE(pop.reading_log_count,0),COALESCE(pop.rating_count,0),p.sequence
                 FROM catalog_publication_subject_assignment a
                 JOIN catalog_source s ON s.source_id=a.source_id AND s.active_generation=a.generation
                 JOIN catalog_publication p ON p.source_id=a.source_id AND p.generation=a.generation AND p.edition_id=a.edition_id AND p.work_id=a.work_id
                 LEFT JOIN catalog_publication_popularity pop
                   ON pop.source_id=p.source_id AND pop.generation=p.generation AND pop.edition_id=p.edition_id AND pop.work_id=p.work_id
                 WHERE a.system_id=?1 AND p.importable=1
                   AND (?2='' OR a.subject_path=?2 OR instr(a.subject_path,?2 || ' / ')=1)
                   AND (?3='' OR instr(p.normalized_title,?3)>0
                     OR EXISTS(SELECT 1 FROM catalog_publication_author author
                       WHERE author.source_id=p.source_id AND author.generation=p.generation
                         AND author.edition_id=p.edition_id AND author.work_id=p.work_id
                         AND instr(author.normalized_name,?3)>0)
                     OR EXISTS(SELECT 1 FROM catalog_publication_toc toc
                       WHERE toc.source_id=p.source_id AND toc.generation=p.generation
                         AND toc.edition_id=p.edition_id AND toc.work_id=p.work_id
                         AND instr(toc.normalized_title,?3)>0))",
        )?;
        let map_row = |row: &rusqlite::Row<'_>| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Vec<u8>>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
            ))
        };
        let mut rows = statement.query_map(params![system_id, location.subject_path.as_deref().unwrap_or(""), normalized_query], map_row)?;
        let mut rejected = HashSet::new();
        while let Some(row) = rows.next() {
            let (system_id, subject_path, source, edition, work, payload, reading_log_count, rating_count, sequence) = row?;
            if !request.source_ids.is_empty() && !request.source_ids.iter().any(|source_id| source_id.as_str() == source) {
                continue;
            }
            let key = (source.clone(), edition, work);
            if rejected.contains(&key) {
                continue;
            }
            if !publications.contains_key(&key) {
                let publication: CatalogPublication = decode_cached(&payload)?;
                if !request.supported_languages.is_empty() && !publication.languages.is_empty() && !publication.languages.iter().any(|language| language_supported(language, &request.supported_languages)) {
                    rejected.insert(key);
                    continue;
                }
                let source_id = CatalogSourceId::parse(&source).map_err(CatalogCacheError::new)?;
                let provider_popularity = project_gutenberg_download_count(&publication).unwrap_or(0);
                popularity.insert(key.clone(), (reading_log_count > 0 || rating_count > 0, reading_log_count, rating_count, provider_popularity, sequence));
                publications.insert(key.clone(), CatalogSourcedPublication { source_id, publication });
            }
            if let Some((child_system, child_path, child_title)) = browse_subject_child(&location, &system_id, &subject_path) {
                child_publications.entry((child_system, child_path, child_title)).or_default().insert(key.clone());
            }
        }

        let mut navigation = child_publications
            .into_iter()
            .map(|((system_id, subject_path, title), keys)| CatalogLink {
                title,
                target: CatalogLinkTarget::Page { locator: subject_locator(&SubjectBrowseLocation { system_id: Some(system_id), subject_path, offset: 0 }) },
                number_of_items: Some(u32::try_from(keys.len()).unwrap_or(u32::MAX)),
                active: false,
            })
            .collect::<Vec<_>>();
        navigation.sort_by(|left, right| left.title.cmp(&right.title));
        if !normalized_query.is_empty() {
            navigation.clear();
        }

        let mut publications = publications.into_values().collect::<Vec<_>>();
        publications.sort_by(|left, right| {
            let left_key = (left.source_id.as_str().to_owned(), left.publication.identity.edition_id.clone(), left.publication.identity.work_id.clone().unwrap_or_default());
            let right_key = (right.source_id.as_str().to_owned(), right.publication.identity.edition_id.clone(), right.publication.identity.work_id.clone().unwrap_or_default());
            popularity
                .get(&right_key)
                .unwrap_or(&(false, 0, 0, 0, 0))
                .cmp(popularity.get(&left_key).unwrap_or(&(false, 0, 0, 0, 0)))
                .then_with(|| left.source_id.as_str().cmp(right.source_id.as_str()))
                .then_with(|| left.publication.identity.edition_id.cmp(&right.publication.identity.edition_id))
        });
        let total = publications.len();
        let publications = publications.into_iter().skip(offset).take(limit).collect();
        let page = offset / limit + 1;
        let page_count = total.div_ceil(limit).max(1);
        let at = |new_offset| subject_locator(&SubjectBrowseLocation { offset: u32::try_from(new_offset).unwrap_or(u32::MAX), ..location.clone() });
        let pagination = CatalogPagination {
            first: (offset > 0).then(|| at(0)),
            previous: (offset > 0).then(|| at(offset.saturating_sub(limit))),
            next: (offset.saturating_add(limit) < total).then(|| at(offset.saturating_add(limit))),
            last: (page < page_count).then(|| at((page_count - 1) * limit)),
            number_of_items: Some(u32::try_from(total).unwrap_or(u32::MAX)),
            items_per_page: Some(u32::try_from(limit).unwrap_or(u32::MAX)),
            current_page: Some(u32::try_from(page).unwrap_or(u32::MAX)),
        };
        Ok(CatalogSubjectBrowseResponse { navigation, publications, pagination })
    }

    pub fn activate_crawl(&self, crawl: &CatalogCrawlGeneration) -> Result<CatalogSourceState, CatalogCacheError> {
        while self.prepare_crawl_activation_batch(crawl, 256)? {}
        let state = self.finalize_crawl_activation(crawl)?;
        while self.reconcile_active_epub_extractions_batch(&crawl.source_id, 256)? {}
        Ok(state)
    }

    /// Assigns stable publication sequences to a bounded slice of a hidden
    /// generation. Each call commits independently so a large catalogue never
    /// monopolizes SQLite's single writer.
    pub fn prepare_crawl_activation_batch(&self, crawl: &CatalogCrawlGeneration, batch_size: usize) -> Result<bool, CatalogCacheError> {
        let batch_size = i64::try_from(batch_size.clamp(1, 1_024)).map_err(|_| CatalogCacheError::new("crawl activation batch size overflow"))?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let active = active_generation_in(&transaction, &crawl.source_id)?;
        let mut next_sequence = next_sequence_in(&transaction, &crawl.source_id)?;
        let generation = storage_i64(crawl.generation, "generation")?;
        let active_generation = storage_i64(active, "generation")?;
        let staged = {
            let mut statement = transaction.prepare(
                "SELECT edition_id,work_id,payload_hash FROM catalog_publication
                 WHERE source_id=?1 AND generation=?2 AND sequence=0
                 ORDER BY edition_id,work_id LIMIT ?3",
            )?;
            let rows = statement.query_map(params![crawl.source_id.as_str(), generation, batch_size], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?.collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (edition_id, work_id, payload_hash) in &staged {
            let previous_sequence = transaction
                .query_row(
                    "SELECT sequence FROM catalog_publication
                     WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 AND payload_hash=?5",
                    params![crawl.source_id.as_str(), active_generation, edition_id, work_id, payload_hash],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            let sequence = match previous_sequence {
                Some(sequence) => sequence,
                None => {
                    next_sequence = next_sequence.checked_add(1).ok_or_else(|| CatalogCacheError::new("publication sequence overflow"))?;
                    storage_i64(next_sequence, "publication sequence")?
                }
            };
            transaction.execute(
                "UPDATE catalog_publication SET sequence=?5
                 WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 AND sequence=0",
                params![crawl.source_id.as_str(), generation, edition_id, work_id, sequence],
            )?;
        }
        transaction.execute("UPDATE catalog_source SET next_sequence=?2 WHERE source_id=?1", params![crawl.source_id.as_str(), storage_i64(next_sequence, "publication sequence")?])?;
        let remaining = staged.len() == usize::try_from(batch_size).unwrap_or(usize::MAX)
            && transaction.query_row("SELECT EXISTS(SELECT 1 FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND sequence=0)", params![crawl.source_id.as_str(), generation], |row| row.get::<_, bool>(0))?;
        transaction.commit()?;
        Ok(remaining)
    }

    /// Performs only the small atomic visibility switch after the hidden
    /// generation has been prepared in bounded commits.
    pub fn finalize_crawl_activation(&self, crawl: &CatalogCrawlGeneration) -> Result<CatalogSourceState, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = storage_i64(crawl.generation, "generation")?;
        let unprepared = transaction.query_row("SELECT EXISTS(SELECT 1 FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND sequence=0)", params![crawl.source_id.as_str(), generation], |row| row.get::<_, bool>(0))?;
        if unprepared {
            return Err(CatalogCacheError::new("crawl generation must be prepared before activation"));
        }

        let (page_count, publication_count, epub_count): (i64, i64, i64) = transaction.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM catalog_page WHERE source_id=?1 AND generation=?2),
                 COUNT(*),
                 COALESCE(SUM(CASE WHEN book_format='epub' AND importable=1 THEN 1 ELSE 0 END), 0)
             FROM catalog_publication WHERE source_id=?1 AND generation=?2",
            params![crawl.source_id.as_str(), generation],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        transaction.execute(
            "UPDATE catalog_source
             SET active_generation=?2, last_crawl_succeeded_at=?3, crawl_status='succeeded', last_crawl_error=NULL,
                 active_page_count=?4, active_publication_count=?5, active_epub_count=?6
             WHERE source_id=?1",
            params![crawl.source_id.as_str(), generation, unix_timestamp()?, page_count, publication_count, epub_count],
        )?;
        let state = source_state_in(&transaction, &crawl.source_id)?.expect("crawl source exists");
        transaction.commit()?;
        Ok(state)
    }

    /// Reconciles a bounded slice of the durable extraction queue with the
    /// active generation. Stale jobs are removed only after every retained
    /// publication has been marked with the new generation.
    pub fn reconcile_active_epub_extractions_batch(&self, source_id: &CatalogSourceId, batch_size: usize) -> Result<bool, CatalogCacheError> {
        let batch_size = i64::try_from(batch_size.clamp(1, 1_024)).map_err(|_| CatalogCacheError::new("extraction reconciliation batch size overflow"))?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = active_generation_in(&transaction, source_id)?;
        let payloads = {
            let mut statement = transaction.prepare(
                "SELECT publication.payload
                 FROM catalog_publication publication
                 LEFT JOIN catalog_epub_extraction extraction
                   ON extraction.source_id=publication.source_id AND extraction.acquisition_url=publication.acquisition_url
                 WHERE publication.source_id=?1 AND publication.generation=?2
                   AND publication.acquisition_url IS NOT NULL AND publication.extraction_format IS NOT NULL
                   AND (extraction.acquisition_url IS NULL OR extraction.catalog_generation IS NOT ?2)
                 ORDER BY publication.edition_id,publication.work_id LIMIT ?3",
            )?;
            let rows = statement.query_map(params![source_id.as_str(), storage_i64(generation, "generation")?, batch_size], |row| row.get::<_, Vec<u8>>(0))?.collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for payload in &payloads {
            let publication: CatalogPublication = decode_cached(payload)?;
            queue_publication_extraction(&transaction, source_id, generation, &publication, false)?;
        }
        let more_publications = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM catalog_publication publication
                 LEFT JOIN catalog_epub_extraction extraction
                   ON extraction.source_id=publication.source_id AND extraction.acquisition_url=publication.acquisition_url
                 WHERE publication.source_id=?1 AND publication.generation=?2
                   AND publication.acquisition_url IS NOT NULL AND publication.extraction_format IS NOT NULL
                   AND (extraction.acquisition_url IS NULL OR extraction.catalog_generation IS NOT ?2)
             )",
            params![source_id.as_str(), storage_i64(generation, "generation")?],
            |row| row.get::<_, bool>(0),
        )?;
        let mut stale_removed = 0;
        if !more_publications {
            stale_removed = transaction.execute(
                "DELETE FROM catalog_epub_extraction WHERE rowid IN (
                     SELECT extraction.rowid FROM catalog_epub_extraction extraction
                     WHERE extraction.source_id=?1 AND NOT EXISTS(
                         SELECT 1 FROM catalog_publication publication
                         WHERE publication.source_id=extraction.source_id AND publication.generation=?2
                           AND publication.acquisition_url=extraction.acquisition_url
                     )
                     LIMIT ?3
                 )",
                params![source_id.as_str(), storage_i64(generation, "generation")?, batch_size],
            )?;
        }
        transaction.commit()?;
        Ok(more_publications || stale_removed == usize::try_from(batch_size).unwrap_or(usize::MAX))
    }

    /// Reclaims inactive generations without monopolizing SQLite's writer
    /// lock. Returns true while more rows remain for a later batch.
    pub fn cleanup_inactive_generation_batch(&self, source_id: &CatalogSourceId, batch_size: usize) -> Result<bool, CatalogCacheError> {
        let batch_size = i64::try_from(batch_size.max(1)).map_err(|_| CatalogCacheError::new("cleanup batch size overflow"))?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let active = storage_i64(active_generation_in(&transaction, source_id)?, "generation")?;
        transaction.execute(
            "DELETE FROM catalog_page WHERE rowid IN (
                 SELECT rowid FROM catalog_page WHERE source_id=?1 AND generation<>?2 LIMIT ?3
             )",
            params![source_id.as_str(), active, batch_size],
        )?;
        transaction.execute(
            "DELETE FROM catalog_publication WHERE rowid IN (
                 SELECT rowid FROM catalog_publication WHERE source_id=?1 AND generation<>?2 LIMIT ?3
             )",
            params![source_id.as_str(), active, batch_size],
        )?;
        let remaining = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM catalog_page WHERE source_id=?1 AND generation<>?2)
                 OR EXISTS(SELECT 1 FROM catalog_publication WHERE source_id=?1 AND generation<>?2)",
            params![source_id.as_str(), active],
            |row| row.get::<_, bool>(0),
        )?;
        transaction.commit()?;
        Ok(remaining)
    }

    pub fn abandon_crawl(&self, crawl: &CatalogCrawlGeneration) -> Result<(), CatalogCacheError> {
        self.abandon_crawl_with_error(crawl, None)
    }

    pub fn abandon_crawl_with_error(&self, crawl: &CatalogCrawlGeneration, error: Option<&str>) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let error = error.map(bounded_crawl_error);
        transaction.execute("UPDATE catalog_source SET crawl_status='failed', last_crawl_error=?2 WHERE source_id=?1", params![crawl.source_id.as_str(), error])?;
        transaction.commit()?;
        Ok(())
    }

    fn put_page_with_sequences(&self, source_id: &CatalogSourceId, source_url: &str, page: &CatalogPage, payloads: &[CatalogPublicationPayload]) -> Result<(), CatalogCacheError> {
        let supplied = payloads.iter().map(|payload| (payload.publication.identity.clone(), payload.sequence)).collect::<HashMap<_, _>>();
        let page_payload = wire::encode(page)?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let generation = active_generation_in(&transaction, source_id)?;
        put_page_row(&transaction, source_id, generation, page, &page_payload, None, None)?;
        for publication in unique_page_publications(page) {
            let sequence =
                supplied.get(&publication.identity).copied().or_else(|| sequence_in(&transaction, source_id, generation, &publication.identity).ok().flatten()).ok_or_else(|| CatalogCacheError::new("publication sequence is unavailable"))?;
            upsert_publication_with_sequence(&transaction, source_id, generation, publication, sequence)?;
        }
        refresh_active_counts(&transaction, source_id, generation)?;
        transaction.commit()?;
        Ok(())
    }
}

fn prepare_database_file(path: &Path) -> Result<(), CatalogCacheError> {
    if !path.exists() {
        return Ok(());
    }
    let current = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .ok()
        .and_then(|connection| {
            let file_version = connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).ok()?;
            let schema_version = connection.query_row("SELECT schema_version FROM catalog_cache_metadata WHERE singleton=1", [], |row| row.get::<_, i64>(0)).ok()?;
            Some(file_version == CATALOG_CACHE_FILE_VERSION && schema_version == CATALOG_CACHE_SCHEMA_VERSION)
        })
        .unwrap_or(false);
    if current {
        return Ok(());
    }
    remove_database_file(path)?;
    Ok(())
}

fn remove_database_file(path: &Path) -> Result<(), CatalogCacheError> {
    for suffix in ["", "-wal", "-shm"] {
        let mut target = path.as_os_str().to_os_string();
        target.push(suffix);
        match fs::remove_file(PathBuf::from(target)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(CatalogCacheError::new(error.to_string())),
        }
    }
    Ok(())
}

fn configure_connection(connection: &mut Connection) -> rusqlite::Result<()> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.busy_timeout(Duration::from_secs(10))
}

fn unix_timestamp() -> Result<i64, CatalogCacheError> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|error| CatalogCacheError::new(error.to_string()))?.as_secs();
    storage_i64(timestamp, "Unix timestamp")
}

fn bounded_crawl_error(error: &str) -> String {
    let mut boundary = error.len().min(MAX_CRAWL_ERROR_BYTES);
    while !error.is_char_boundary(boundary) {
        boundary -= 1;
    }
    error[..boundary].to_owned()
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
    transaction.execute("DELETE FROM catalog_epub_extraction WHERE source_id=?1", [source_id.as_str()])?;
    transaction.execute(
        "UPDATE catalog_source SET epoch=?2, active_generation=1, next_sequence=0, crawl_status='never', last_crawl_error=NULL,
             active_page_count=0, active_publication_count=0, active_epub_count=0
         WHERE source_id=?1",
        params![source_id.as_str(), Uuid::new_v4().to_string()],
    )?;
    Ok(())
}

fn refresh_active_counts(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64) -> Result<(), CatalogCacheError> {
    let generation = storage_i64(generation, "generation")?;
    transaction.execute(
        "UPDATE catalog_source
         SET active_page_count=(SELECT COUNT(*) FROM catalog_page WHERE source_id=?1 AND generation=?2),
             active_publication_count=(SELECT COUNT(*) FROM catalog_publication WHERE source_id=?1 AND generation=?2),
             active_epub_count=(SELECT COUNT(*) FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND book_format='epub' AND importable=1)
         WHERE source_id=?1",
        params![source_id.as_str(), generation],
    )?;
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

fn put_page_row(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, page: &CatalogPage, payload: &[u8], etag: Option<&str>, last_modified: Option<&str>) -> Result<(), CatalogCacheError> {
    transaction.execute(
        "INSERT INTO catalog_page(source_id, generation, locator, payload, etag, last_modified) VALUES(?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(source_id, generation, locator) DO UPDATE SET payload=excluded.payload, etag=excluded.etag, last_modified=excluded.last_modified",
        params![source_id.as_str(), storage_i64(generation, "generation")?, page.locator, payload, etag, last_modified],
    )?;
    replace_page_links(transaction, source_id.as_str(), storage_i64(generation, "generation")?, &page.locator, page)?;
    Ok(())
}

fn replace_page_links(transaction: &Transaction<'_>, source_id: &str, generation: i64, locator: &str, page: &CatalogPage) -> Result<(), CatalogCacheError> {
    transaction.execute("DELETE FROM catalog_page_link WHERE source_id=?1 AND generation=?2 AND from_locator=?3", params![source_id, generation, locator])?;
    let mut seen = HashSet::new();
    let navigation = page.navigation.iter().chain(page.groups.iter().flat_map(|group| group.navigation.iter())).filter_map(|link| match &link.target {
        CatalogLinkTarget::Page { locator } => Some(locator.as_str()),
        CatalogLinkTarget::External { .. } => None,
    });
    for target in navigation
        .chain(page.groups.iter().filter_map(|group| group.page_locator.as_deref()))
        .chain([page.pagination.first.as_deref(), page.pagination.previous.as_deref(), page.pagination.next.as_deref(), page.pagination.last.as_deref()].into_iter().flatten())
    {
        if seen.insert(target) {
            transaction.execute("INSERT OR IGNORE INTO catalog_page_link(source_id,generation,from_locator,to_locator) VALUES(?1,?2,?3,?4)", params![source_id, generation, locator, target])?;
        }
    }
    Ok(())
}

fn replace_publication_subjects(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, identity: &CatalogPublicationIdentity, subjects: &[library_model::PublicationSubject]) -> Result<(), CatalogCacheError> {
    let generation = storage_i64(generation, "generation")?;
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute("DELETE FROM catalog_publication_subject_assignment WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, identity.edition_id, work_id])?;
    transaction.execute("DELETE FROM catalog_publication_subject WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, identity.edition_id, work_id])?;
    for (position, subject) in subjects.iter().enumerate() {
        transaction.execute(
            "INSERT INTO catalog_publication_subject(source_id,generation,edition_id,work_id,position,name,source,authority,code) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                source_id.as_str(),
                generation,
                identity.edition_id,
                work_id,
                i64::try_from(position).map_err(|_| CatalogCacheError::new("subject position overflow"))?,
                subject.name(),
                subject.source(),
                subject.authority(),
                subject.code()
            ],
        )?;
    }
    insert_subject_assignments(transaction, source_id.as_str(), generation, &identity.edition_id, work_id, subjects)?;
    Ok(())
}

fn replace_publication_toc(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, identity: &CatalogPublicationIdentity, toc: &[library_model::PublicationTocEntry]) -> Result<(), CatalogCacheError> {
    let generation = storage_i64(generation, "generation")?;
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute("DELETE FROM catalog_publication_toc WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4", params![source_id.as_str(), generation, identity.edition_id, work_id])?;
    let mut titles = Vec::new();
    flatten_toc_titles(toc, &mut titles);
    for (position, title) in titles.into_iter().enumerate() {
        let normalized = normalize_search_text(title);
        if normalized.is_empty() {
            continue;
        }
        transaction.execute(
            "INSERT INTO catalog_publication_toc(source_id,generation,edition_id,work_id,position,normalized_title) VALUES(?1,?2,?3,?4,?5,?6)",
            params![source_id.as_str(), generation, identity.edition_id, work_id, i64::try_from(position).map_err(|_| CatalogCacheError::new("TOC position overflow"))?, normalized],
        )?;
    }
    Ok(())
}

fn flatten_toc_titles<'a>(entries: &'a [library_model::PublicationTocEntry], output: &mut Vec<&'a str>) {
    for entry in entries {
        output.push(&entry.title);
        flatten_toc_titles(&entry.children, output);
    }
}

fn insert_subject_assignments(transaction: &Transaction<'_>, source_id: &str, generation: i64, edition_id: &str, work_id: &str, subjects: &[library_model::PublicationSubject]) -> Result<(), CatalogCacheError> {
    for assignment in library_model::project_subjects(subjects) {
        transaction.execute(
            "INSERT INTO catalog_publication_subject_assignment(source_id,generation,edition_id,work_id,system_id,subject_path,matcher_version) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![source_id, generation, edition_id, work_id, assignment.system_id(), assignment.subject_path(), assignment.matcher_version()],
        )?;
    }
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

struct PublicationInventory {
    book_format: Option<&'static str>,
    importable: bool,
    has_upstream_cover: bool,
}

struct PublicationExtraction<'a> {
    acquisition_url: &'a str,
    book_format: CatalogBookFormat,
}

struct PreparedStagedPublication<'a> {
    publication: &'a CatalogPublication,
    payload: Vec<u8>,
    payload_hash: String,
    normalized_title: String,
    inventory: PublicationInventory,
    extraction: Option<PublicationExtraction<'a>>,
}

impl<'a> PreparedStagedPublication<'a> {
    fn new(publication: &'a CatalogPublication) -> Result<Self, CatalogCacheError> {
        let payload = wire::encode(publication)?;
        let payload_hash = blake3::hash(&payload).to_hex().to_string();
        Ok(Self { publication, payload, payload_hash, normalized_title: normalize_search_text(&publication.title), inventory: publication_inventory(publication), extraction: publication_extraction(publication) })
    }
}

fn publication_extraction(publication: &CatalogPublication) -> Option<PublicationExtraction<'_>> {
    publication.actions.iter().find_map(|action| match action {
        CatalogPublicationAction::Import { request } if matches!(request.book_format, CatalogBookFormat::Epub | CatalogBookFormat::Pdf | CatalogBookFormat::Mobi) => {
            Some(PublicationExtraction { acquisition_url: &request.download_url, book_format: request.book_format })
        }
        _ => None,
    })
}

fn publication_inventory(publication: &CatalogPublication) -> PublicationInventory {
    let book_format = publication.actions.iter().find_map(|action| match action {
        CatalogPublicationAction::Import { request } => Some(book_format_storage(request.book_format)),
        CatalogPublicationAction::External { .. } => None,
    });
    PublicationInventory { book_format, importable: book_format.is_some(), has_upstream_cover: publication.image_url.as_deref().is_some_and(|url| url.starts_with("https://") || url.starts_with("http://")) }
}

fn book_format_storage(format: CatalogBookFormat) -> &'static str {
    match format {
        CatalogBookFormat::Epub => "epub",
        CatalogBookFormat::Pdf => "pdf",
        CatalogBookFormat::Mobi => "mobi",
        CatalogBookFormat::Fb2 => "fb2",
        CatalogBookFormat::Cbz => "cbz",
        CatalogBookFormat::WebPub => "webpub",
        CatalogBookFormat::M4b => "m4b",
    }
}

fn catalog_book_format_from_storage(value: &str) -> Option<CatalogBookFormat> {
    match value {
        "epub" => Some(CatalogBookFormat::Epub),
        "pdf" => Some(CatalogBookFormat::Pdf),
        "mobi" => Some(CatalogBookFormat::Mobi),
        "fb2" => Some(CatalogBookFormat::Fb2),
        "cbz" => Some(CatalogBookFormat::Cbz),
        "webpub" => Some(CatalogBookFormat::WebPub),
        "m4b" => Some(CatalogBookFormat::M4b),
        _ => None,
    }
}

fn queue_publication_extraction(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, publication: &CatalogPublication, force: bool) -> Result<(), CatalogCacheError> {
    let generation = storage_i64(generation, "generation")?;
    let work_id = publication.identity.work_id.as_deref().unwrap_or("");
    let extraction = publication_extraction(publication);
    transaction.execute(
        "DELETE FROM catalog_epub_extraction
         WHERE source_id=?1 AND edition_id=?2 AND work_id=?3
           AND acquisition_url IS NOT ?4",
        params![source_id.as_str(), publication.identity.edition_id, work_id, extraction.as_ref().map(|item| item.acquisition_url)],
    )?;
    let Some(extraction) = extraction else {
        return Ok(());
    };
    transaction.execute(
        "INSERT INTO catalog_epub_extraction(
             source_id, acquisition_url, status, enrichment_version,
             edition_id, work_id, book_format, cover_key, catalog_generation
         ) VALUES(?1, ?2, 'pending', ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(source_id, acquisition_url) DO UPDATE SET
             status=CASE
                 WHEN catalog_epub_extraction.status='running' THEN 'running'
                 WHEN ?9
                   OR catalog_epub_extraction.enrichment_version!=excluded.enrichment_version
                   OR catalog_epub_extraction.edition_id IS NOT excluded.edition_id
                   OR catalog_epub_extraction.work_id IS NOT excluded.work_id
                   OR catalog_epub_extraction.book_format IS NOT excluded.book_format
                   OR catalog_epub_extraction.cover_key IS NOT excluded.cover_key
                   OR (catalog_epub_extraction.status='succeeded' AND catalog_epub_extraction.result_payload IS NULL)
                 THEN 'pending' ELSE catalog_epub_extraction.status END,
             phase=CASE
                 WHEN catalog_epub_extraction.status='running' THEN catalog_epub_extraction.phase
                 WHEN ?9
                   OR catalog_epub_extraction.enrichment_version!=excluded.enrichment_version
                   OR catalog_epub_extraction.edition_id IS NOT excluded.edition_id
                   OR catalog_epub_extraction.work_id IS NOT excluded.work_id
                   OR catalog_epub_extraction.book_format IS NOT excluded.book_format
                   OR catalog_epub_extraction.cover_key IS NOT excluded.cover_key
                   OR catalog_epub_extraction.result_payload IS NULL
                 THEN NULL ELSE catalog_epub_extraction.phase END,
             last_error=CASE
                 WHEN ?9 OR catalog_epub_extraction.enrichment_version!=excluded.enrichment_version
                   OR catalog_epub_extraction.result_payload IS NULL
                 THEN NULL ELSE catalog_epub_extraction.last_error END,
             enrichment_version=excluded.enrichment_version,
             edition_id=excluded.edition_id,
             work_id=excluded.work_id,
             book_format=excluded.book_format,
             cover_key=excluded.cover_key,
             catalog_generation=excluded.catalog_generation",
        params![source_id.as_str(), extraction.acquisition_url, CATALOG_EPUB_ENRICHMENT_VERSION, publication.identity.edition_id, work_id, book_format_storage(extraction.book_format), publication.image_url, generation, force,],
    )?;
    Ok(())
}

fn upsert_active_publication(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, publication: &CatalogPublication) -> Result<(), CatalogCacheError> {
    let payload = wire::encode(publication)?;
    let payload_hash = blake3::hash(&payload).to_hex().to_string();
    let inventory = publication_inventory(publication);
    let extraction = publication_extraction(publication);
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
        "INSERT INTO catalog_publication(source_id, generation, edition_id, work_id, sequence, payload_hash, normalized_title, book_format, importable, has_upstream_cover, inventory_indexed, acquisition_url, extraction_format, cover_url, payload)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, ?12, ?13, ?14)
         ON CONFLICT(source_id, generation, edition_id, work_id) DO UPDATE SET sequence=excluded.sequence, payload_hash=excluded.payload_hash, normalized_title=excluded.normalized_title,
             book_format=excluded.book_format, importable=excluded.importable, has_upstream_cover=excluded.has_upstream_cover, inventory_indexed=1,
             acquisition_url=excluded.acquisition_url, extraction_format=excluded.extraction_format, cover_url=excluded.cover_url, payload=excluded.payload",
        params![
            source_id.as_str(),
            storage_i64(generation, "generation")?,
            edition_id,
            work_id,
            sequence,
            payload_hash,
            normalize_search_text(&publication.title),
            inventory.book_format,
            inventory.importable,
            inventory.has_upstream_cover,
            extraction.as_ref().map(|item| item.acquisition_url),
            extraction.as_ref().map(|item| book_format_storage(item.book_format)),
            publication.image_url,
            payload
        ],
    )?;
    replace_publication_metadata(transaction, source_id, generation, publication)?;
    Ok(())
}

fn upsert_publication_with_sequence(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, publication: &CatalogPublication, sequence: u64) -> Result<(), CatalogCacheError> {
    let payload = wire::encode(publication)?;
    let payload_hash = blake3::hash(&payload).to_hex().to_string();
    let inventory = publication_inventory(publication);
    let extraction = publication_extraction(publication);
    let (edition_id, work_id) = publication_key(publication);
    let sequence = storage_i64(sequence, "publication sequence")?;
    transaction.execute(
        "INSERT INTO catalog_publication(source_id, generation, edition_id, work_id, sequence, payload_hash, normalized_title, book_format, importable, has_upstream_cover, inventory_indexed, acquisition_url, extraction_format, cover_url, payload)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, ?12, ?13, ?14)
         ON CONFLICT(source_id, generation, edition_id, work_id) DO UPDATE SET sequence=excluded.sequence, payload_hash=excluded.payload_hash, normalized_title=excluded.normalized_title,
             book_format=excluded.book_format, importable=excluded.importable, has_upstream_cover=excluded.has_upstream_cover, inventory_indexed=1,
             acquisition_url=excluded.acquisition_url, extraction_format=excluded.extraction_format, cover_url=excluded.cover_url, payload=excluded.payload",
        params![
            source_id.as_str(),
            storage_i64(generation, "generation")?,
            edition_id,
            work_id,
            sequence,
            payload_hash,
            normalize_search_text(&publication.title),
            inventory.book_format,
            inventory.importable,
            inventory.has_upstream_cover,
            extraction.as_ref().map(|item| item.acquisition_url),
            extraction.as_ref().map(|item| book_format_storage(item.book_format)),
            publication.image_url,
            payload
        ],
    )?;
    transaction.execute("UPDATE catalog_source SET next_sequence=MAX(next_sequence, ?2) WHERE source_id=?1", params![source_id.as_str(), sequence])?;
    replace_publication_metadata(transaction, source_id, generation, publication)
}

fn insert_staged_publication(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, prepared: &PreparedStagedPublication<'_>) -> Result<(), CatalogCacheError> {
    let publication = prepared.publication;
    let (edition_id, work_id) = publication_key(publication);
    let inserted = transaction.execute(
        "INSERT OR IGNORE INTO catalog_publication(source_id, generation, edition_id, work_id, sequence, payload_hash, normalized_title, book_format, importable, has_upstream_cover, inventory_indexed, acquisition_url, extraction_format, cover_url, payload)
         VALUES(?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8, ?9, 1, ?10, ?11, ?12, ?13)",
        params![
            source_id.as_str(), storage_i64(generation, "generation")?, edition_id, work_id, prepared.payload_hash, prepared.normalized_title, prepared.inventory.book_format,
            prepared.inventory.importable, prepared.inventory.has_upstream_cover, prepared.extraction.as_ref().map(|item| item.acquisition_url), prepared.extraction.as_ref().map(|item| book_format_storage(item.book_format)),
            publication.image_url, prepared.payload
        ],
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

fn copy_subject_metadata(transaction: &Transaction<'_>, source_id: &CatalogSourceId, from_generation: u64, to_generation: u64, identity: &CatalogPublicationIdentity) -> Result<(), CatalogCacheError> {
    if from_generation == to_generation {
        return Ok(());
    }
    let from_generation = storage_i64(from_generation, "generation")?;
    let to_generation = storage_i64(to_generation, "generation")?;
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute(
        "INSERT OR IGNORE INTO catalog_publication_subject(source_id,generation,edition_id,work_id,position,name,source,authority,code)
         SELECT source_id,?2,edition_id,work_id,position,name,source,authority,code FROM catalog_publication_subject
         WHERE source_id=?1 AND generation=?3 AND edition_id=?4 AND work_id=?5",
        params![source_id.as_str(), to_generation, from_generation, identity.edition_id, work_id],
    )?;
    transaction.execute(
        "INSERT OR IGNORE INTO catalog_publication_subject_assignment(source_id,generation,edition_id,work_id,system_id,subject_path,matcher_version)
         SELECT source_id,?2,edition_id,work_id,system_id,subject_path,matcher_version FROM catalog_publication_subject_assignment
         WHERE source_id=?1 AND generation=?3 AND edition_id=?4 AND work_id=?5",
        params![source_id.as_str(), to_generation, from_generation, identity.edition_id, work_id],
    )?;
    Ok(())
}

fn copy_toc_metadata(transaction: &Transaction<'_>, source_id: &CatalogSourceId, from_generation: u64, to_generation: u64, identity: &CatalogPublicationIdentity) -> Result<(), CatalogCacheError> {
    if from_generation == to_generation {
        return Ok(());
    }
    let from_generation = storage_i64(from_generation, "generation")?;
    let to_generation = storage_i64(to_generation, "generation")?;
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute(
        "INSERT OR IGNORE INTO catalog_publication_toc(source_id,generation,edition_id,work_id,position,normalized_title)
         SELECT source_id,?2,edition_id,work_id,position,normalized_title FROM catalog_publication_toc
         WHERE source_id=?1 AND generation=?3 AND edition_id=?4 AND work_id=?5",
        params![source_id.as_str(), to_generation, from_generation, identity.edition_id, work_id],
    )?;
    Ok(())
}

fn copy_popularity_metadata(transaction: &Transaction<'_>, source_id: &CatalogSourceId, from_generation: u64, to_generation: u64, identity: &CatalogPublicationIdentity) -> Result<(), CatalogCacheError> {
    if from_generation == to_generation {
        return Ok(());
    }
    let from_generation = storage_i64(from_generation, "generation")?;
    let to_generation = storage_i64(to_generation, "generation")?;
    let work_id = identity.work_id.as_deref().unwrap_or("");
    transaction.execute(
        "INSERT OR IGNORE INTO catalog_publication_popularity(
            source_id,generation,edition_id,work_id,want_to_read_count,currently_reading_count,already_read_count,reading_log_count,rating_count,rating_sum
         ) SELECT source_id,?2,edition_id,work_id,want_to_read_count,currently_reading_count,already_read_count,reading_log_count,rating_count,rating_sum
           FROM catalog_publication_popularity
          WHERE source_id=?1 AND generation=?3 AND edition_id=?4 AND work_id=?5",
        params![source_id.as_str(), to_generation, from_generation, identity.edition_id, work_id],
    )?;
    Ok(())
}

fn valid_subject_key(system_id: &str, subject_path: &str) -> bool {
    !system_id.trim().is_empty()
        && system_id.len() <= 32
        && system_id.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !subject_path.trim().is_empty()
        && subject_path.len() <= 1_024
        && !subject_path.chars().any(char::is_control)
}

fn browse_subject_roots(connection: &Connection, request: &CatalogSubjectBrowseRequest) -> Result<CatalogSubjectBrowseResponse, CatalogCacheError> {
    let mut primary_languages = request.supported_languages.iter().filter_map(|language| language.split('-').next()).filter(|language| !language.is_empty()).map(str::to_ascii_lowercase).collect::<Vec<_>>();
    primary_languages.sort();
    primary_languages.dedup();
    let language_predicate = if request.supported_languages.is_empty() {
        String::new()
    } else if primary_languages.is_empty() {
        " AND NOT EXISTS(SELECT 1 FROM catalog_publication_language l WHERE l.source_id=p.source_id AND l.generation=p.generation AND l.edition_id=p.edition_id AND l.work_id=p.work_id)".to_owned()
    } else {
        let placeholders = std::iter::repeat_n("?", primary_languages.len()).collect::<Vec<_>>().join(",");
        format!(
            " AND (NOT EXISTS(SELECT 1 FROM catalog_publication_language l WHERE l.source_id=p.source_id AND l.generation=p.generation AND l.edition_id=p.edition_id AND l.work_id=p.work_id)
               OR EXISTS(SELECT 1 FROM catalog_publication_language l WHERE l.source_id=p.source_id AND l.generation=p.generation AND l.edition_id=p.edition_id AND l.work_id=p.work_id
                         AND lower(CASE WHEN instr(l.language,'-')>0 THEN substr(l.language,1,instr(l.language,'-')-1) ELSE l.language END) IN ({placeholders})))"
        )
    };
    let sql = format!(
        "SELECT a.system_id,p.source_id,p.edition_id,p.work_id
         FROM catalog_publication_subject_assignment a
         JOIN catalog_source s ON s.source_id=a.source_id AND s.active_generation=a.generation
         JOIN catalog_publication p ON p.source_id=a.source_id AND p.generation=a.generation AND p.edition_id=a.edition_id AND p.work_id=a.work_id
         WHERE a.system_id IN ('bisac','ddc','lcc') AND p.importable=1{language_predicate}"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(primary_languages.iter()), |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?)))?;
    let mut counts = HashMap::<String, HashSet<(String, String, String)>>::new();
    for row in rows {
        let (system_id, source_id, edition_id, work_id) = row?;
        if !request.source_ids.is_empty() && !request.source_ids.iter().any(|selected| selected.as_str() == source_id) {
            continue;
        }
        counts.entry(system_id).or_default().insert((source_id, edition_id, work_id));
    }
    let mut navigation = counts
        .into_iter()
        .map(|(system_id, publications)| CatalogLink {
            title: subject_system_title(&system_id).to_owned(),
            target: CatalogLinkTarget::Page { locator: subject_locator(&SubjectBrowseLocation { subject_path: subject_system_root(&system_id), system_id: Some(system_id), offset: 0 }) },
            number_of_items: Some(u32::try_from(publications.len()).unwrap_or(u32::MAX)),
            active: false,
        })
        .collect::<Vec<_>>();
    navigation.sort_by(|left, right| left.title.cmp(&right.title));
    Ok(CatalogSubjectBrowseResponse { navigation, publications: Vec::new(), pagination: CatalogPagination::default() })
}

#[derive(Clone, Debug, Default)]
struct SubjectBrowseLocation {
    system_id: Option<String>,
    subject_path: Option<String>,
    offset: u32,
}

fn browse_subject_system(system_id: &str) -> bool {
    matches!(system_id, library_model::BISAC_SYSTEM_ID | library_model::DDC_SYSTEM_ID | library_model::LCC_SYSTEM_ID)
}

fn subject_system_title(system_id: &str) -> &'static str {
    match system_id {
        library_model::BISAC_SYSTEM_ID => "BISAC",
        library_model::DDC_SYSTEM_ID => "Dewey Decimal Classification",
        library_model::LCC_SYSTEM_ID => "Library of Congress Classification",
        _ => "Subjects",
    }
}

fn subject_system_root(system_id: &str) -> Option<String> {
    match system_id {
        library_model::DDC_SYSTEM_ID => Some("DDC".to_owned()),
        library_model::LCC_SYSTEM_ID => Some("LCC".to_owned()),
        _ => None,
    }
}

fn browse_subject_child(location: &SubjectBrowseLocation, system_id: &str, assigned_path: &str) -> Option<(String, Option<String>, String)> {
    if location.system_id.is_none() {
        return browse_subject_system(system_id).then(|| (system_id.to_owned(), subject_system_root(system_id), subject_system_title(system_id).to_owned()));
    }
    let base = location.subject_path.as_deref().unwrap_or("");
    let remainder = if base.is_empty() { assigned_path } else { assigned_path.strip_prefix(base)?.strip_prefix(" / ")? };
    let child = remainder.split(" / ").next()?.trim();
    if child.is_empty() {
        return None;
    }
    let child_path = if base.is_empty() { child.to_owned() } else { format!("{base} / {child}") };
    Some((system_id.to_owned(), Some(child_path), child.to_owned()))
}

fn subject_locator(location: &SubjectBrowseLocation) -> String {
    let mut locator = url::Url::parse("bokheim:subjects").expect("static catalogue subject locator must be valid");
    {
        let mut query = locator.query_pairs_mut();
        if let Some(system_id) = &location.system_id {
            query.append_pair("system", system_id);
        }
        if let Some(subject_path) = &location.subject_path {
            query.append_pair("path", subject_path);
        }
        if location.offset > 0 {
            query.append_pair("offset", &location.offset.to_string());
        }
    }
    locator.into()
}

fn parse_subject_locator(locator: &str) -> Result<SubjectBrowseLocation, CatalogCacheError> {
    if locator.len() > 2_048 {
        return Err(CatalogCacheError::new("catalogue subject locator is too long"));
    }
    let parsed = url::Url::parse(locator).map_err(|_| CatalogCacheError::new("invalid catalogue subject locator"))?;
    if parsed.scheme() != "bokheim" || parsed.path() != "subjects" || parsed.fragment().is_some() {
        return Err(CatalogCacheError::new("invalid catalogue subject locator"));
    }
    let mut location = SubjectBrowseLocation::default();
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "system" if location.system_id.is_none() => location.system_id = Some(value.into_owned().to_ascii_lowercase()),
            "path" if location.subject_path.is_none() => location.subject_path = Some(value.into_owned()),
            "offset" if location.offset == 0 => location.offset = value.parse().map_err(|_| CatalogCacheError::new("invalid catalogue subject offset"))?,
            _ => return Err(CatalogCacheError::new("invalid catalogue subject locator")),
        }
    }
    let Some(system_id) = location.system_id.as_deref() else { return Err(CatalogCacheError::new("catalogue subject locator is missing its system")) };
    if !browse_subject_system(system_id) || location.subject_path.as_deref().is_some_and(|path| !valid_subject_key(system_id, path)) {
        return Err(CatalogCacheError::new("invalid catalogue subject locator"));
    }
    Ok(location)
}

fn subject_distance(left: &str, right: &str) -> usize {
    let left = left.split(" / ").count();
    let right = right.split(" / ").count();
    left.abs_diff(right)
}

fn language_supported(publication_language: &str, supported: &[String]) -> bool {
    supported
        .iter()
        .any(|supported| publication_language.eq_ignore_ascii_case(supported) || publication_language.split('-').next().is_some_and(|primary| supported.split('-').next().is_some_and(|candidate| primary.eq_ignore_ascii_case(candidate))))
}

fn recommendation_dedupe_key(recommendation: &CatalogRecommendation) -> String {
    let title = normalize_search_text(&recommendation.publication.title);
    let author = recommendation.publication.authors.first().map(|author| normalize_search_text(&author.name)).unwrap_or_default();
    if !title.is_empty() && !author.is_empty() {
        format!("bibliographic\n{title}\n{author}")
    } else {
        format!("identity\n{}\n{}\n{}", recommendation.source_id.as_str(), recommendation.publication.identity.edition_id, recommendation.publication.identity.work_id.as_deref().unwrap_or(""))
    }
}

fn allocate_sequence(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<i64, CatalogCacheError> {
    let next = next_sequence_in(transaction, source_id)?.checked_add(1).ok_or_else(|| CatalogCacheError::new("publication sequence overflow"))?;
    transaction.execute("UPDATE catalog_source SET next_sequence=?2 WHERE source_id=?1", params![source_id.as_str(), storage_i64(next, "publication sequence")?])?;
    storage_i64(next, "publication sequence")
}

fn decode_sequenced_publication(row: &rusqlite::Row<'_>) -> rusqlite::Result<SequencedCatalogPublication> {
    let payload: Vec<u8> = row.get(0)?;
    let publication = decode_cached(&payload).map_err(sql_decode_error)?;
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

fn sequenced_publication_with_popularity_in(connection: &Connection, source_id: &CatalogSourceId, generation: u64, identity: &CatalogPublicationIdentity) -> Result<Option<(SequencedCatalogPublication, (u64, u64))>, CatalogCacheError> {
    connection
        .query_row(
            "SELECT publication.payload, publication.sequence,
                    COALESCE(popularity.reading_log_count,0), COALESCE(popularity.rating_count,0)
             FROM catalog_publication publication
             LEFT JOIN catalog_publication_popularity popularity
               ON popularity.source_id=publication.source_id AND popularity.generation=publication.generation
              AND popularity.edition_id=publication.edition_id AND popularity.work_id=publication.work_id
             WHERE publication.source_id=?1 AND publication.generation=?2
               AND publication.edition_id=?3 AND publication.work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, identity.edition_id, identity.work_id.as_deref().unwrap_or("")],
            |row| {
                let publication = decode_sequenced_publication(row)?;
                let reading_log_count = stored_u64(row.get(2)?, "reading-log count")?;
                let rating_count = stored_u64(row.get(3)?, "rating count")?;
                Ok((publication, (reading_log_count, rating_count)))
            },
        )
        .optional()
        .map_err(Into::into)
}

fn sort_page_publications_by_popularity(page: &mut CatalogPage, popularity: &HashMap<CatalogPublicationIdentity, (u64, u64)>) {
    let sort = |publications: &mut Vec<CatalogPublication>| {
        // `sort_by` is stable, so equally popular and unenriched books retain
        // the catalogue provider's order.
        publications.sort_by(|left, right| popularity.get(&right.identity).unwrap_or(&(0, 0)).cmp(popularity.get(&left.identity).unwrap_or(&(0, 0))));
    };
    sort(&mut page.publications);
    for group in &mut page.groups {
        sort(&mut group.publications);
    }
}

fn project_gutenberg_download_count(publication: &CatalogPublication) -> Option<u64> {
    if !publication.identity.edition_id.starts_with("urn:gutenberg:") {
        return None;
    }
    let description = match publication.description.as_ref()? {
        CatalogDescription::PlainText { text } => text.as_str(),
        CatalogDescription::RichHtml { plain_text, .. } => plain_text.as_str(),
    };
    let value = description.rsplit_once("Downloads:")?.1.trim_start();
    let digits = value.chars().take_while(char::is_ascii_digit).collect::<String>();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
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
    decode_cached(payload)
}

fn decode_cached<T: serde::de::DeserializeOwned>(payload: &[u8]) -> Result<T, wire::WireError> {
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

fn optional_stored_u64(value: Option<i64>, name: &str) -> rusqlite::Result<Option<u64>> {
    value.map(|value| stored_u64(value, name)).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog_contract::{CatalogAuthorCredit, CatalogFreshness, CatalogPagination, CatalogPublicationIdentity, CatalogRecommendationExclusion, CatalogRecommendationRequest, CatalogSubjectPreference};
    use library_model::PublicationSubject;

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

    fn importable_epub(id: &str, image_url: Option<&str>) -> CatalogPublication {
        let mut publication = publication(id, "EPUB", "Author", &["en"]);
        publication.image_url = image_url.map(str::to_owned);
        publication.actions.push(CatalogPublicationAction::Import {
            request: catalog_contract::CatalogImportRequest {
                source_id: source_id(),
                catalog_locator: "https://example.com/opds".to_owned(),
                publication: publication.identity.clone(),
                download_url: format!("https://example.com/{id}.epub"),
                book_format: CatalogBookFormat::Epub,
            },
        });
        publication
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
    fn local_search_returns_distinct_table_of_contents_matches() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        let book = publication("toc", "Collected Essays", "Ada Author", &["en"]);
        cache.put_page(&source_id(), "https://example.com/opds", &page("https://example.com/opds", vec![book.clone()]), None, None).unwrap();
        let toc = vec![library_model::PublicationTocEntry {
            title: "Industrial transformation".to_owned(),
            target: "chapter.xhtml".to_owned(),
            children: vec![library_model::PublicationTocEntry { title: "Swedish railways".to_owned(), target: "chapter.xhtml#rail".to_owned(), children: Vec::new() }],
        }];
        assert!(cache.enrich_active_publication(&source_id(), &book, &[], &toc, None).unwrap());

        let matches = cache.search(&source_id(), "railways").unwrap();
        assert!(matches.title_matches.is_empty());
        assert!(matches.author_matches.is_empty());
        assert_eq!(matches.toc_matches[0].publication.identity.edition_id, "toc");
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

    #[test]
    fn page_response_sorts_books_by_open_library_popularity_and_preserves_ties() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        let locator = "https://example.com/opds";
        let first = publication("first", "First", "Author", &["en"]);
        let popular = publication("popular", "Popular", "Author", &["en"]);
        let third = publication("third", "Third", "Author", &["en"]);
        cache.put_page(&source_id(), locator, &page(locator, vec![first, popular.clone(), third]), None, None).unwrap();
        cache.enrich_active_publication(&source_id(), &popular, &[], &[], Some(&CatalogPublicationPopularity { want_to_read_count: 20, currently_reading_count: 3, already_read_count: 7, rating_count: 4, rating_sum: 18 })).unwrap();

        let response = cache.page_response(&source_id(), locator, None, &[]).unwrap().unwrap();
        assert_eq!(response.page.publications.iter().map(|publication| publication.identity.edition_id.as_str()).collect::<Vec<_>>(), vec!["popular", "first", "third"]);
    }

    #[test]
    fn replaces_an_outdated_cache_instead_of_migrating_it() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("catalog-cache.db");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "PRAGMA user_version=1;
                 CREATE TABLE migration_sentinel(value TEXT NOT NULL);
                 INSERT INTO migration_sentinel VALUES('preserved');",
            )
            .unwrap();
        drop(connection);

        let cache = CatalogCache::open(&path).unwrap();
        let inventory = cache.inventory().unwrap();
        assert_eq!(inventory.active_page_count, 0);
        assert_eq!(inventory.active_publication_count, 0);
        assert_eq!(inventory.active_epub_count, 0);
        drop(cache);

        let connection = Connection::open(&path).unwrap();
        assert_eq!(connection.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0)).unwrap(), CATALOG_CACHE_FILE_VERSION);
        assert_eq!(connection.query_row("SELECT schema_version FROM catalog_cache_metadata WHERE singleton=1", [], |row| row.get::<_, i64>(0)).unwrap(), CATALOG_CACHE_SCHEMA_VERSION);
        assert!(connection.query_row("SELECT value FROM migration_sentinel", [], |row| row.get::<_, String>(0)).is_err());
    }

    #[test]
    fn current_cache_adds_failure_diagnostics_without_discarding_inventory() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("catalog-cache.db");
        {
            let cache = CatalogCache::open(&path).unwrap();
            cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
        }
        let connection = Connection::open(&path).unwrap();
        connection.execute("DROP TABLE catalog_epub_failure", ()).unwrap();
        drop(connection);

        let cache = CatalogCache::open(&path).unwrap();
        assert_eq!(cache.inventory().unwrap().sources.len(), 1);
        let connection = Connection::open(&path).unwrap();
        assert!(connection.query_row("SELECT COUNT(*) FROM catalog_epub_failure", (), |row| row.get::<_, i64>(0)).is_ok());
    }

    #[test]
    fn crawl_status_and_counts_change_atomically_without_losing_active_inventory() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
        let crawl = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
        assert_eq!(cache.inventory().unwrap().sources[0].crawl_status, "running");
        cache.stage_page(&crawl, &page("https://example.com/opds", vec![importable_epub("one", None)]), None, None).unwrap();
        cache.activate_crawl(&crawl).unwrap();

        let succeeded = cache.inventory().unwrap();
        assert_eq!((succeeded.active_page_count, succeeded.active_publication_count, succeeded.active_epub_count), (1, 1, 1));
        assert_eq!(succeeded.sources[0].crawl_status, "succeeded");
        assert!(succeeded.sources[0].last_crawl_started_at.is_some());
        assert!(succeeded.sources[0].last_crawl_succeeded_at.is_some());

        let failed = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
        cache.stage_page(&failed, &page("https://example.com/opds", Vec::new()), None, None).unwrap();
        cache.abandon_crawl_with_error(&failed, Some("upstream timeout")).unwrap();
        let inventory = cache.inventory().unwrap();
        assert_eq!((inventory.active_page_count, inventory.active_publication_count, inventory.active_epub_count), (1, 1, 1));
        assert_eq!(inventory.sources[0].crawl_status, "failed");
        assert_eq!(inventory.sources[0].last_crawl_error.as_deref(), Some("upstream timeout"));
    }

    #[test]
    fn crawl_activation_and_extraction_reconciliation_are_bounded() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        let source = source_id();
        cache.ensure_source(&source, "https://example.com/opds").unwrap();
        let crawl = cache.begin_crawl(&source, "https://example.com/opds").unwrap();
        cache.stage_page(&crawl, &page("https://example.com/opds", vec![importable_epub("one", None), importable_epub("two", None), importable_epub("three", None)]), None, None).unwrap();

        assert!(cache.prepare_crawl_activation_batch(&crawl, 1).unwrap());
        assert!(cache.prepare_crawl_activation_batch(&crawl, 1).unwrap());
        assert!(!cache.prepare_crawl_activation_batch(&crawl, 1).unwrap());
        cache.finalize_crawl_activation(&crawl).unwrap();

        assert!(cache.reconcile_active_epub_extractions_batch(&source, 1).unwrap());
        assert!(cache.reconcile_active_epub_extractions_batch(&source, 1).unwrap());
        assert!(!cache.reconcile_active_epub_extractions_batch(&source, 1).unwrap());
        assert_eq!(cache.inventory().unwrap().epub_extractions.pending, 3);
    }

    #[test]
    fn incremental_crawl_publishes_pages_and_preserves_partial_progress() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        let source = source_id();
        cache.begin_incremental_crawl(&source, "https://example.com/opds").unwrap();
        cache.put_page(&source, "https://example.com/opds", &page("https://example.com/opds", vec![importable_epub("one", None)]), None, None).unwrap();

        let running = cache.inventory().unwrap();
        assert_eq!((running.active_page_count, running.active_publication_count), (1, 1));
        assert_eq!(running.sources[0].crawl_status, "running");

        cache.finish_incremental_crawl(&source, false).unwrap();
        let partial = cache.inventory().unwrap();
        assert_eq!((partial.active_page_count, partial.active_publication_count), (1, 1));
        assert_eq!(partial.sources[0].crawl_status, "partial");
        assert!(cache.page(&source, "https://example.com/opds").unwrap().is_some());
    }

    #[test]
    fn epub_extraction_inventory_persists_and_requeues_interrupted_work() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("catalog-cache.db");
        let source = source_id();
        let succeeded = "https://example.com/succeeded.epub".to_owned();
        let failed = "https://example.com/failed.epub".to_owned();
        let interrupted = "https://example.com/interrupted.epub".to_owned();

        {
            let cache = CatalogCache::open(&path).unwrap();
            cache.ensure_source(&source, "https://example.com/opds").unwrap();
            cache.queue_crawls(std::slice::from_ref(&source)).unwrap();
            assert_eq!(cache.inventory().unwrap().sources[0].crawl_status, "queued");
            cache.queue_epub_extractions(&source, &[succeeded.clone(), failed.clone(), interrupted.clone()], false).unwrap();
            assert!(cache.begin_epub_extraction(&source, &succeeded).unwrap());
            cache.finish_epub_extraction(&source, &succeeded, 12_345, &publication("one", "One", "Author", &["en"]), None).unwrap();
            cache.begin_epub_extraction(&source, &failed).unwrap();
            cache.fail_epub_extraction(&source, &failed, "invalid EPUB", "invalid_publication", false).unwrap();
            cache.begin_epub_extraction(&source, &interrupted).unwrap();
            let extraction = &cache.inventory().unwrap().sources[0].epub_extractions;
            assert_eq!((extraction.pending, extraction.running, extraction.succeeded, extraction.failed), (0, 1, 1, 1));
        }

        let reopened = CatalogCache::open(&path).unwrap();
        assert_eq!(reopened.inventory().unwrap().sources[0].crawl_status, "queued");
        let extraction = &reopened.inventory().unwrap().sources[0].epub_extractions;
        assert_eq!((extraction.pending, extraction.running, extraction.succeeded, extraction.failed), (1, 0, 1, 1));
        assert_eq!(extraction.attempts, 3);
        assert_eq!(extraction.downloaded_bytes, 12_345);
        assert_eq!(reopened.successful_epub_extractions(&source).unwrap(), HashSet::from([succeeded]));
        assert_eq!(reopened.pending_epub_extractions(&source).unwrap(), HashSet::from([interrupted]));
        let failures = reopened.epub_failure_diagnostics().unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!((failures[0].category.as_str(), failures[0].retryable, failures[0].count), ("invalid_publication", false, 1));
        assert_eq!(failures[0].examples[0].acquisition_host, "example.com");
        reopened.retry_failed_epub_extractions(&source).unwrap();
        assert_eq!(reopened.inventory().unwrap().epub_extractions.failed, 1, "permanent failures must not be blindly requeued");
    }

    #[test]
    fn saved_metadata_backfill_is_durable_and_does_not_requeue_downloads() {
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("catalog-cache.db");
        let source = source_id();
        let book = importable_epub("saved", None);
        {
            let cache = CatalogCache::open(&path).unwrap();
            cache.put_page(&source, "https://example.com/opds", &page("https://example.com/opds", vec![book.clone()]), None, None).unwrap();
            let extraction = cache.claim_next_epub_extraction(&source).unwrap().unwrap();
            let metadata =
                CatalogEpubMetadata { book: library_model::BookMetadataState { title: "Saved".to_owned(), contributors: Vec::new(), description: String::new(), publication: library_model::PublicationMetadata::default() }, toc: Vec::new() };
            cache.finish_epub_extraction(&source, &extraction.acquisition_url, 123, &book, Some(&metadata)).unwrap();
            assert_eq!(cache.schedule_metadata_backfill().unwrap(), 1);
            assert_eq!(cache.claim_metadata_backfill_batch(128).unwrap().len(), 1);
            assert_eq!(cache.inventory().unwrap().epub_extractions.attempts, 1);
        }

        let cache = CatalogCache::open(&path).unwrap();
        let jobs = cache.claim_metadata_backfill_batch(128).unwrap();
        assert_eq!(jobs.len(), 1, "an interrupted metadata-only batch must resume after restart");
        cache.finish_metadata_backfill_batch(&[CatalogMetadataBackfillUpdate { source_id: source.clone(), publication: book.identity, acquisition_url: jobs[0].acquisition_url.clone(), subjects: Vec::new(), popularity: None }]).unwrap();
        assert!(cache.claim_metadata_backfill_batch(128).unwrap().is_empty());
        assert_eq!(cache.inventory().unwrap().epub_extractions.attempts, 1, "metadata backfill must not claim or download the publication again");
    }

    #[test]
    fn durable_extraction_queue_claims_bounded_jobs_and_discards_stale_acquisitions() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        let source = source_id();
        let first = importable_epub("one", Some("urn:cover:one"));
        let second = importable_epub("two", None);
        cache.put_page(&source, "https://example.com/opds", &page("https://example.com/opds", vec![first.clone(), second.clone()]), None, None).unwrap();

        let claimed = cache.claim_next_epub_extraction(&source).unwrap().unwrap();
        assert_eq!(claimed.publication, first.identity);
        assert_eq!(claimed.acquisition_url, "https://example.com/one.epub");
        assert_eq!(claimed.book_format, CatalogBookFormat::Epub);
        assert_eq!(claimed.cover_key.as_deref(), Some("urn:cover:one"));
        let inventory = cache.inventory().unwrap().epub_extractions;
        assert_eq!((inventory.pending, inventory.running), (1, 1));

        let crawl = cache.begin_crawl(&source, "https://example.com/opds").unwrap();
        cache.stage_page(&crawl, &page("https://example.com/opds", vec![second.clone()]), None, None).unwrap();
        cache.activate_crawl(&crawl).unwrap();

        assert_eq!(cache.epub_extraction_status(&source, "https://example.com/one.epub").unwrap(), None);
        let remaining = cache.claim_next_epub_extraction(&source).unwrap().unwrap();
        assert_eq!(remaining.publication, second.identity);
        assert!(cache.claim_next_epub_extraction(&source).unwrap().is_none());
    }

    #[test]
    fn page_graph_frontier_and_enrichment_are_resolved_without_loading_all_pages() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        let source = source_id();
        let root_url = "https://example.com/opds";
        let child_url = "https://example.com/child";
        let mut root = page(root_url, vec![importable_epub("one", None)]);
        root.navigation.push(CatalogLink { title: "Child".to_owned(), target: CatalogLinkTarget::Page { locator: child_url.to_owned() }, number_of_items: None, active: false });
        cache.put_page(&source, root_url, &root, None, None).unwrap();
        assert_eq!(cache.missing_page_frontier(&source, root_url).unwrap(), [child_url]);

        let claimed = cache.claim_next_epub_extraction(&source).unwrap().unwrap();
        let mut enriched = importable_epub("one", None);
        enriched.title = "Embedded title".to_owned();
        cache.finish_epub_extraction(&source, &claimed.acquisition_url, 10, &enriched, None).unwrap();
        let mut refreshed_root = root.clone();
        cache.enrich_page_from_epub_extractions(&source, &mut refreshed_root).unwrap();
        assert_eq!(refreshed_root.publications[0].title, "Embedded title");

        cache.put_page(&source, root_url, &page(child_url, Vec::new()), None, None).unwrap();
        assert!(cache.missing_page_frontier(&source, root_url).unwrap().is_empty());
    }

    #[test]
    fn epub_extraction_inventory_discards_urls_no_longer_in_the_catalogue() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        let source = source_id();
        let retained = "https://example.com/retained.epub".to_owned();
        let removed = "https://example.com/removed.epub".to_owned();
        cache.ensure_source(&source, "https://example.com/opds").unwrap();
        cache.queue_epub_extractions(&source, &[retained.clone(), removed], false).unwrap();
        cache.retain_epub_extractions(&source, &HashSet::from([retained])).unwrap();

        let extraction = &cache.inventory().unwrap().sources[0].epub_extractions;
        assert_eq!((extraction.pending, extraction.running, extraction.succeeded, extraction.failed), (1, 0, 0, 0));
    }

    #[test]
    fn subject_browse_reuses_projected_hierarchy_and_paginates_sourced_books() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
        let crawl = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
        let books = page("https://example.com/opds", vec![importable_epub("history-one", None), importable_epub("history-two", None)]);
        cache.stage_page(&crawl, &books, None, None).unwrap();
        let history = PublicationSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("bisac".to_owned()), Some("HIS036060".to_owned())).unwrap();
        let dewey = PublicationSubject::new(None, "515.33", "openlibrary", Some("ddc".to_owned()), Some("515.33".to_owned())).unwrap();
        cache.stage_publication_subjects(&crawl, &books.publications[0].identity, &[history.clone(), dewey]).unwrap();
        cache.stage_publication_subjects(&crawl, &books.publications[1].identity, &[history]).unwrap();
        cache.activate_crawl(&crawl).unwrap();
        cache
            .enrich_active_publication(
                &source_id(),
                &books.publications[1],
                &[PublicationSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("bisac".to_owned()), Some("HIS036060".to_owned())).unwrap()],
                &[],
                Some(&CatalogPublicationPopularity { want_to_read_count: 40, currently_reading_count: 2, already_read_count: 8, rating_count: 3, rating_sum: 13 }),
            )
            .unwrap();

        let root = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: None, supported_languages: vec!["en".to_owned()], source_ids: Vec::new(), query: String::new(), limit: 1 }).unwrap();
        assert_eq!(root.navigation.iter().map(|link| link.title.as_str()).collect::<Vec<_>>(), ["BISAC", "Dewey Decimal Classification"]);
        assert!(root.publications.is_empty());
        let bisac = root.navigation.iter().find(|link| link.title == "BISAC").unwrap();
        let CatalogLinkTarget::Page { locator } = &bisac.target else { panic!("subject roots must be internal links") };
        let first = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: Some(locator.clone()), supported_languages: vec!["en".to_owned()], source_ids: Vec::new(), query: String::new(), limit: 1 }).unwrap();
        assert_eq!(first.navigation[0].title, "History");
        assert_eq!(first.publications.len(), 1);
        assert_eq!(first.publications[0].source_id, source_id());
        assert_eq!(first.publications[0].publication.identity.edition_id, "history-two", "popular books sort first within an equally relevant subject");
        assert_eq!(first.pagination.number_of_items, Some(2));
        let second = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: first.pagination.next.clone(), supported_languages: vec!["en".to_owned()], source_ids: Vec::new(), query: String::new(), limit: 1 }).unwrap();
        assert_eq!(second.publications.len(), 1);
        assert_ne!(first.publications[0].publication.identity, second.publications[0].publication.identity);

        let excluded_source = CatalogSourceId::parse("another-source").unwrap();
        let filtered_root = cache.browse_subjects(&CatalogSubjectBrowseRequest {
            locator: None,
            supported_languages: vec!["en".to_owned()],
            source_ids: vec![excluded_source],
            query: String::new(),
            limit: 10,
        });
        assert!(filtered_root.unwrap().navigation.is_empty());

        let searched = cache.browse_subjects(&CatalogSubjectBrowseRequest {
            locator: Some(locator.clone()),
            supported_languages: vec!["en".to_owned()],
            source_ids: vec![source_id()],
            query: "HISTORY-ONE".to_owned(),
            limit: 10,
        });
        let searched = searched.unwrap();
        assert!(searched.navigation.is_empty());
        assert_eq!(searched.publications.len(), 1);
        assert_eq!(searched.publications[0].publication.identity.edition_id, "history-one");
    }

    #[test]
    fn subject_browse_uses_gutenberg_downloads_when_open_library_popularity_is_missing() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
        let crawl = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
        let mut alphabetical_first = importable_epub("urn:gutenberg:1:1", None);
        alphabetical_first.title = "A lesser-known book".to_owned();
        alphabetical_first.description = Some(CatalogDescription::PlainText { text: "Downloads: 12 Language: English".to_owned() });
        let mut popular = importable_epub("urn:gutenberg:2:1", None);
        popular.title = "The popular book".to_owned();
        popular.description = Some(CatalogDescription::PlainText { text: "Downloads: 45000 Language: English".to_owned() });
        let books = page("https://example.com/opds", vec![alphabetical_first, popular]);
        cache.stage_page(&crawl, &books, None, None).unwrap();
        let fiction = PublicationSubject::new(None, "Fiction / General", "dc:subject", Some("bisac".to_owned()), Some("FIC000000".to_owned())).unwrap();
        for publication in &books.publications {
            cache.stage_publication_subjects(&crawl, &publication.identity, std::slice::from_ref(&fiction)).unwrap();
        }
        cache.activate_crawl(&crawl).unwrap();

        let roots = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: None, supported_languages: vec!["en".to_owned()], source_ids: Vec::new(), query: String::new(), limit: 10 }).unwrap();
        let bisac = roots.navigation.iter().find(|link| link.title == "BISAC").unwrap();
        let CatalogLinkTarget::Page { locator } = &bisac.target else { panic!("subject roots must be internal links") };
        let response = cache.browse_subjects(&CatalogSubjectBrowseRequest { locator: Some(locator.clone()), supported_languages: vec!["en".to_owned()], source_ids: Vec::new(), query: String::new(), limit: 10 }).unwrap();
        assert_eq!(response.publications[0].publication.title, "The popular book");
    }

    #[test]
    fn canonical_subject_recommendations_rank_filter_exclude_and_survive_recrawl() {
        let temporary = tempfile::tempdir().unwrap();
        let cache = CatalogCache::open(temporary.path().join("catalog-cache.db")).unwrap();
        cache.ensure_source(&source_id(), "https://example.com/opds").unwrap();
        let first = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
        let books = page("https://example.com/opds", vec![importable_epub("history-en", None), importable_epub("history-sv", None), importable_epub("fiction", None)]);
        cache.stage_page(&first, &books, None, None).unwrap();
        let history = PublicationSubject::new(None, "History / United States / 20th Century", "dc:subject", Some("bisac".to_owned()), Some("HIS036060".to_owned())).unwrap();
        let fiction = PublicationSubject::new(None, "Fiction / General", "dc:subject", Some("bisac".to_owned()), Some("FIC000000".to_owned())).unwrap();
        cache.stage_publication_subjects(&first, &books.publications[0].identity, std::slice::from_ref(&history)).unwrap();
        cache.stage_publication_subjects(&first, &books.publications[1].identity, std::slice::from_ref(&history)).unwrap();
        cache.stage_publication_subjects(&first, &books.publications[2].identity, std::slice::from_ref(&fiction)).unwrap();
        cache.activate_crawl(&first).unwrap();

        let request = CatalogRecommendationRequest {
            subjects: vec![CatalogSubjectPreference { system_id: "bisac".to_owned(), subject_path: "History / United States".to_owned(), weight: 5 }],
            supported_languages: vec!["en".to_owned()],
            excluded_publications: vec![CatalogRecommendationExclusion { source_id: source_id(), publication: books.publications[1].identity.clone() }],
            limit: 10,
        };
        let recommendations = cache.recommendations(&request).unwrap().recommendations;
        assert_eq!(recommendations.len(), 1);
        assert_eq!(recommendations[0].publication.identity.edition_id, "history-en");
        assert_eq!(recommendations[0].matched_subjects[0].subject_path, "History / United States / 20th Century");

        let second = cache.begin_crawl(&source_id(), "https://example.com/opds").unwrap();
        cache.stage_page(&second, &books, None, None).unwrap();
        cache.activate_crawl(&second).unwrap();
        assert_eq!(cache.recommendations(&request).unwrap().recommendations.len(), 1, "subject sidecars are copied into an unchanged generation");
    }
}
