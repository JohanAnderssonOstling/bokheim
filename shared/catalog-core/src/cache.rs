use catalog_contract::{
    CatalogBookFormat, CatalogBookFormatFacetValue, CatalogDescription, CatalogLanguageFacetValue, CatalogLink, CatalogLinkTarget, CatalogPage, CatalogPublication, CatalogPublicationAction, CatalogPublicationIdentity, CatalogSourceId,
    CatalogSourcedPublication, CatalogSubjectBrowseCapabilities, CatalogSubjectBrowseCapabilitiesResponse, CatalogSubjectBrowseDeltaResponse, CatalogSubjectBrowseEntryPayload, CatalogSubjectBrowseRefreshRequest,
    CatalogSubjectBrowseRequest, CatalogSubjectBrowseResponse, CatalogSubjectCollectionManifestRequest, CatalogSubjectCollectionManifestResponse, CatalogSubjectPublicationPayloadRequest, CatalogSubjectPublicationPayloadResponse,
    CatalogSubjectSort, PublicationSubject,
};
use catalog_model::normalize_search_text;
#[cfg(not(target_arch = "wasm32"))]
use r2d2::Pool;
#[cfg(not(target_arch = "wasm32"))]
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
struct CatalogPool(Pool<SqliteConnectionManager>);

#[cfg(not(target_arch = "wasm32"))]
impl CatalogPool {
    fn get(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, CatalogCacheError> {
        self.0.get().map_err(Into::into)
    }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
struct CatalogPool(std::rc::Rc<std::cell::RefCell<Connection>>);

#[cfg(target_arch = "wasm32")]
impl CatalogPool {
    fn get(&self) -> Result<std::cell::RefMut<'_, Connection>, CatalogCacheError> {
        self.0.try_borrow_mut().map_err(|_| CatalogCacheError::new("catalogue cache connection is already borrowed"))
    }
}

use catalog_contract::{materialize_page, page_layout, CatalogPageResponse, CatalogPublicationPayload, CatalogPublicationReference, CatalogSearchResponse};

// Keep SQLite's file version at 1 because the additive v2 schema remains
// writable by the previous binary during an automated deployment rollback.
// The logical version lives in catalog_cache_metadata instead.
const CATALOG_CACHE_FILE_VERSION: i64 = 1;
// Version 25 records whether a logical page was compacted from provider
// pagination so replacement ingestion knows when every upstream page must be
// fetched again.
// Version 24 stores locator-scoped subject collections for local querying.
// Version 23 adopts shared sourced title/property evidence in persisted EPUB
// enrichment payloads.
// Version 22 stores the Gutenberg edition preference used to canonicalize
// works before subject facets, ordering, totals, and range selection.
// Version 21 adopts the shared rich subject representation.
// Version 20 adopts the shared bibliographic collection representation.
// Version 19 adopts the shared bibliographic identifier representation.
// Version 18 stored provider-native popularity alongside normalized
// publication inventory so catalogue ordering is complete before range
// selection.
// The cache is rehydratable, so replacing it is safer than attempting to
// decode an incompatible persisted representation during startup.
#[cfg(not(target_arch = "wasm32"))]
const CATALOG_CACHE_SCHEMA_VERSION: i64 = 25;
const CATALOG_EPUB_ENRICHMENT_VERSION: i64 = 5;
const CATALOG_METADATA_BACKFILL_VERSION: i64 = 2;
// Version 4 keeps this pass separate from regular enrichment while also
// applying validated work-subject supplements from the description sidecar.
const CATALOG_DESCRIPTION_BACKFILL_VERSION: i64 = 4;
const CATALOG_METADATA_INDEX_VERSION: i64 = 1;
const CATALOG_SEARCH_INDEX_VERSION: i64 = 1;
const CATALOG_SUBJECT_BROWSE_INDEX_VERSION: i64 = 1;
const CATALOG_BROWSE_SERVING_INDEX_VERSION: i64 = 1;
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
INSERT OR IGNORE INTO catalog_cache_metadata(singleton, schema_version) VALUES(1, 25);

CREATE TABLE IF NOT EXISTS catalog_transport_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    epoch TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS catalog_process_control (
    process TEXT PRIMARY KEY CHECK(process IN ('catalogue_crawling','cover_downloads','epub_enrichment','catalogue_enrichment')),
    enabled INTEGER NOT NULL CHECK(enabled IN (0, 1))
);
INSERT OR IGNORE INTO catalog_process_control(process,enabled) VALUES
    ('catalogue_crawling',0),
    ('cover_downloads',0),
    ('epub_enrichment',0),
    ('catalogue_enrichment',0);

CREATE TABLE IF NOT EXISTS catalog_cover_download (
    source_id TEXT NOT NULL REFERENCES catalog_source(source_id) ON DELETE CASCADE,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    image_url TEXT NOT NULL,
    priority INTEGER NOT NULL,
    catalog_generation INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','running','succeeded','failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    last_attempt_at INTEGER,
    last_succeeded_at INTEGER,
    last_error TEXT,
    PRIMARY KEY(source_id, edition_id, work_id, image_url)
);
CREATE INDEX IF NOT EXISTS catalog_cover_download_status
    ON catalog_cover_download(status, attempts, source_id, edition_id, work_id, image_url);

CREATE TABLE IF NOT EXISTS catalog_subject_publication_cache (
    epoch TEXT NOT NULL,
    entry_id INTEGER NOT NULL,
    source_id TEXT NOT NULL,
    normalized_title TEXT NOT NULL,
    normalized_authors TEXT NOT NULL,
    normalized_search_terms TEXT NOT NULL,
    languages TEXT NOT NULL,
    formats TEXT NOT NULL,
    publication_payload BLOB NOT NULL,
    PRIMARY KEY(epoch, entry_id)
);

CREATE TABLE IF NOT EXISTS catalog_subject_collection (
    locator TEXT PRIMARY KEY,
    epoch TEXT NOT NULL,
    revision INTEGER NOT NULL,
    navigation_payload BLOB NOT NULL,
    stored_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS catalog_subject_collection_member (
    locator TEXT NOT NULL REFERENCES catalog_subject_collection(locator) ON DELETE CASCADE,
    entry_id INTEGER NOT NULL,
    popularity_position INTEGER NOT NULL,
    exact_assignment INTEGER NOT NULL CHECK(exact_assignment IN (0, 1)),
    PRIMARY KEY(locator, entry_id)
);
CREATE INDEX IF NOT EXISTS catalog_subject_collection_position
    ON catalog_subject_collection_member(locator, exact_assignment, popularity_position, entry_id);

CREATE TABLE IF NOT EXISTS catalog_subject_collection_sync (
    locator TEXT PRIMARY KEY,
    epoch TEXT NOT NULL,
    revision INTEGER NOT NULL,
    navigation_payload BLOB NOT NULL,
    total_items INTEGER NOT NULL,
    next_offset INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS catalog_subject_collection_staging (
    locator TEXT NOT NULL,
    entry_id INTEGER NOT NULL,
    popularity_position INTEGER NOT NULL,
    exact_assignment INTEGER NOT NULL CHECK(exact_assignment IN (0, 1)),
    PRIMARY KEY(locator, entry_id)
);

CREATE TABLE IF NOT EXISTS catalog_subject_projection_metadata (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    unified_matcher_version INTEGER NOT NULL
);

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

CREATE TABLE IF NOT EXISTS catalog_description_backfill (
    source_id TEXT NOT NULL,
    acquisition_url TEXT NOT NULL,
    version INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'running', 'succeeded')),
    attempts INTEGER NOT NULL DEFAULT 0,
    last_attempt_at INTEGER,
    last_error TEXT,
    outcome TEXT CHECK(outcome IS NULL OR outcome = 'no_description'),
    PRIMARY KEY(source_id, acquisition_url),
    FOREIGN KEY(source_id, acquisition_url)
        REFERENCES catalog_epub_extraction(source_id, acquisition_url)
        ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS catalog_description_backfill_status
    ON catalog_description_backfill(status, attempts, source_id, acquisition_url);

CREATE TABLE IF NOT EXISTS catalog_metadata_index (
    source_id TEXT NOT NULL,
    acquisition_url TEXT NOT NULL,
    version INTEGER NOT NULL,
    title TEXT NOT NULL,
    publication_year INTEGER,
    authors_payload BLOB NOT NULL,
    publishers_payload BLOB NOT NULL,
    isbns_payload BLOB NOT NULL,
    PRIMARY KEY(source_id, acquisition_url),
    FOREIGN KEY(source_id, acquisition_url)
        REFERENCES catalog_epub_extraction(source_id, acquisition_url)
        ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS catalog_metadata_index_version
    ON catalog_metadata_index(version, source_id, acquisition_url);

CREATE TABLE IF NOT EXISTS catalog_identity_review (
    review_id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL,
    acquisition_url TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('ambiguous','no_match','insufficient_metadata','failed','resolved','rejected')),
    result_payload BLOB,
    detail TEXT,
    candidates_truncated INTEGER NOT NULL DEFAULT 0 CHECK(candidates_truncated IN (0,1)),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    resolved_at INTEGER,
    selected_isbn TEXT,
    UNIQUE(source_id, acquisition_url),
    FOREIGN KEY(source_id, acquisition_url)
        REFERENCES catalog_epub_extraction(source_id, acquisition_url)
        ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS catalog_identity_review_status
    ON catalog_identity_review(status, source_id, updated_at, review_id);

CREATE TABLE IF NOT EXISTS catalog_page (
    source_id TEXT NOT NULL REFERENCES catalog_source(source_id) ON DELETE CASCADE,
    generation INTEGER NOT NULL,
    locator TEXT NOT NULL,
    payload BLOB NOT NULL,
    etag TEXT,
    last_modified TEXT,
    provider_paginated INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(source_id, generation, locator)
);

CREATE TABLE IF NOT EXISTS catalog_publication_entry (
    entry_id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id TEXT NOT NULL REFERENCES catalog_source(source_id) ON DELETE CASCADE,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    payload_hash TEXT NOT NULL,
    UNIQUE(source_id, edition_id, work_id, payload_hash)
);

CREATE TABLE IF NOT EXISTS catalog_publication (
    source_id TEXT NOT NULL REFERENCES catalog_source(source_id) ON DELETE CASCADE,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    entry_id INTEGER NOT NULL REFERENCES catalog_publication_entry(entry_id),
    sequence INTEGER NOT NULL,
    payload_hash TEXT NOT NULL,
    normalized_title TEXT NOT NULL,
    provider_popularity INTEGER NOT NULL DEFAULT 0,
    gutenberg_has_images INTEGER NOT NULL DEFAULT 0 CHECK(gutenberg_has_images IN (0, 1)),
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

CREATE TABLE IF NOT EXISTS catalog_publication_format (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    book_format TEXT NOT NULL,
    PRIMARY KEY(source_id, generation, edition_id, work_id, book_format),
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

-- Subject browsing uses a materialized hierarchy rather than deriving parent
-- and child paths from every assignment at request time. A membership row is
-- unique per publication and ancestor node, so navigation counts are exact
-- COUNT(*) aggregates without a temporary DISTINCT publication set.
CREATE TABLE IF NOT EXISTS catalog_subject_node (
    system_id TEXT NOT NULL,
    node_path TEXT NOT NULL,
    parent_path TEXT NOT NULL,
    title TEXT NOT NULL,
    PRIMARY KEY(system_id, node_path)
);

CREATE TABLE IF NOT EXISTS catalog_publication_subject_node (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    system_id TEXT NOT NULL,
    node_path TEXT NOT NULL,
    exact_assignment INTEGER NOT NULL CHECK(exact_assignment IN (0, 1)),
    PRIMARY KEY(source_id, generation, edition_id, work_id, system_id, node_path),
    FOREIGN KEY(source_id, generation, edition_id, work_id)
        REFERENCES catalog_publication(source_id, generation, edition_id, work_id)
        ON DELETE CASCADE,
    FOREIGN KEY(system_id, node_path)
        REFERENCES catalog_subject_node(system_id, node_path)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS catalog_subject_browse_index_metadata (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    index_version INTEGER NOT NULL
);

-- Store requests use a disposable, revisioned projection with one integer
-- identity per active canonical publication. The ingestion tables above stay
-- authoritative; this projection can always be rebuilt atomically.
CREATE TABLE IF NOT EXISTS catalog_browse_serving_metadata (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    index_version INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    ready INTEGER NOT NULL CHECK(ready IN (0, 1))
);
INSERT OR IGNORE INTO catalog_browse_serving_metadata(singleton,index_version,revision,ready)
    VALUES(1,1,0,0);

CREATE TABLE IF NOT EXISTS catalog_browse_publication (
    entry_id INTEGER PRIMARY KEY,
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    normalized_title TEXT NOT NULL,
    provider_popularity INTEGER NOT NULL,
    sequence INTEGER NOT NULL,
    reading_log_count INTEGER NOT NULL,
    rating_count INTEGER NOT NULL,
    has_language INTEGER NOT NULL CHECK(has_language IN (0, 1))
);
CREATE INDEX IF NOT EXISTS catalog_browse_publication_source
    ON catalog_browse_publication(source_id, entry_id);
CREATE INDEX IF NOT EXISTS catalog_browse_publication_title
    ON catalog_browse_publication(normalized_title, entry_id);
CREATE INDEX IF NOT EXISTS catalog_browse_publication_popularity
    ON catalog_browse_publication(
        (reading_log_count>0 OR rating_count>0) DESC,
        reading_log_count DESC,
        rating_count DESC,
        provider_popularity DESC,
        sequence DESC,
        normalized_title,
        entry_id
    );

CREATE TABLE IF NOT EXISTS catalog_browse_search_document (
    entry_id INTEGER PRIMARY KEY,
    search_text TEXT NOT NULL
);
CREATE VIRTUAL TABLE IF NOT EXISTS catalog_browse_search_fts USING fts5(
    search_text,
    content='catalog_browse_search_document',
    content_rowid='entry_id',
    tokenize='trigram'
);

CREATE TABLE IF NOT EXISTS catalog_browse_language (
    entry_id INTEGER NOT NULL,
    language TEXT NOT NULL,
    PRIMARY KEY(entry_id, language)
);
CREATE INDEX IF NOT EXISTS catalog_browse_language_value
    ON catalog_browse_language(language, entry_id);

CREATE TABLE IF NOT EXISTS catalog_browse_format (
    entry_id INTEGER NOT NULL,
    book_format TEXT NOT NULL,
    PRIMARY KEY(entry_id, book_format)
);
CREATE INDEX IF NOT EXISTS catalog_browse_format_value
    ON catalog_browse_format(book_format, entry_id);

CREATE TABLE IF NOT EXISTS catalog_browse_subject (
    entry_id INTEGER NOT NULL,
    system_id TEXT NOT NULL,
    node_path TEXT NOT NULL,
    exact_assignment INTEGER NOT NULL CHECK(exact_assignment IN (0, 1)),
    PRIMARY KEY(entry_id, system_id, node_path)
);
CREATE INDEX IF NOT EXISTS catalog_browse_subject_node
    ON catalog_browse_subject(system_id, node_path, entry_id, exact_assignment);

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

-- These external-content FTS tables index the existing normalized search
-- columns without duplicating publication payloads. Triggers keep the index
-- correct even if this database is temporarily opened by the rollback binary.
CREATE VIRTUAL TABLE IF NOT EXISTS catalog_publication_title_fts USING fts5(
    normalized_title,
    content='catalog_publication',
    content_rowid='rowid',
    tokenize='trigram'
);
CREATE VIRTUAL TABLE IF NOT EXISTS catalog_publication_author_fts USING fts5(
    normalized_name,
    content='catalog_publication_author',
    content_rowid='rowid',
    tokenize='trigram'
);
CREATE VIRTUAL TABLE IF NOT EXISTS catalog_publication_toc_fts USING fts5(
    normalized_title,
    content='catalog_publication_toc',
    content_rowid='rowid',
    tokenize='trigram'
);
CREATE TABLE IF NOT EXISTS catalog_search_index_metadata (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    index_version INTEGER NOT NULL,
    ready INTEGER NOT NULL CHECK(ready IN (0, 1))
);
INSERT OR IGNORE INTO catalog_search_index_metadata(singleton,index_version,ready) VALUES(1,1,0);

CREATE TRIGGER IF NOT EXISTS catalog_publication_title_fts_insert AFTER INSERT ON catalog_publication BEGIN
    INSERT INTO catalog_publication_title_fts(rowid,normalized_title) VALUES(new.rowid,new.normalized_title);
END;
CREATE TRIGGER IF NOT EXISTS catalog_publication_title_fts_delete AFTER DELETE ON catalog_publication BEGIN
    INSERT INTO catalog_publication_title_fts(catalog_publication_title_fts,rowid,normalized_title) VALUES('delete',old.rowid,old.normalized_title);
END;
CREATE TRIGGER IF NOT EXISTS catalog_publication_title_fts_update AFTER UPDATE OF normalized_title ON catalog_publication BEGIN
    INSERT INTO catalog_publication_title_fts(catalog_publication_title_fts,rowid,normalized_title) VALUES('delete',old.rowid,old.normalized_title);
    INSERT INTO catalog_publication_title_fts(rowid,normalized_title) VALUES(new.rowid,new.normalized_title);
END;

CREATE TRIGGER IF NOT EXISTS catalog_publication_author_fts_insert AFTER INSERT ON catalog_publication_author BEGIN
    INSERT INTO catalog_publication_author_fts(rowid,normalized_name) VALUES(new.rowid,new.normalized_name);
END;
CREATE TRIGGER IF NOT EXISTS catalog_publication_author_fts_delete AFTER DELETE ON catalog_publication_author BEGIN
    INSERT INTO catalog_publication_author_fts(catalog_publication_author_fts,rowid,normalized_name) VALUES('delete',old.rowid,old.normalized_name);
END;
CREATE TRIGGER IF NOT EXISTS catalog_publication_author_fts_update AFTER UPDATE OF normalized_name ON catalog_publication_author BEGIN
    INSERT INTO catalog_publication_author_fts(catalog_publication_author_fts,rowid,normalized_name) VALUES('delete',old.rowid,old.normalized_name);
    INSERT INTO catalog_publication_author_fts(rowid,normalized_name) VALUES(new.rowid,new.normalized_name);
END;

CREATE TRIGGER IF NOT EXISTS catalog_publication_toc_fts_insert AFTER INSERT ON catalog_publication_toc BEGIN
    INSERT INTO catalog_publication_toc_fts(rowid,normalized_title) VALUES(new.rowid,new.normalized_title);
END;
CREATE TRIGGER IF NOT EXISTS catalog_publication_toc_fts_delete AFTER DELETE ON catalog_publication_toc BEGIN
    INSERT INTO catalog_publication_toc_fts(catalog_publication_toc_fts,rowid,normalized_title) VALUES('delete',old.rowid,old.normalized_title);
END;
CREATE TRIGGER IF NOT EXISTS catalog_publication_toc_fts_update AFTER UPDATE OF normalized_title ON catalog_publication_toc BEGIN
    INSERT INTO catalog_publication_toc_fts(catalog_publication_toc_fts,rowid,normalized_title) VALUES('delete',old.rowid,old.normalized_title);
    INSERT INTO catalog_publication_toc_fts(rowid,normalized_title) VALUES(new.rowid,new.normalized_title);
END;

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
CREATE INDEX IF NOT EXISTS catalog_publication_entry_lookup
    ON catalog_publication(entry_id, source_id, generation);
CREATE INDEX IF NOT EXISTS catalog_publication_author_name
    ON catalog_publication_author(source_id, generation, normalized_name, edition_id, work_id);
CREATE INDEX IF NOT EXISTS catalog_publication_language_tag
    ON catalog_publication_language(source_id, generation, language, edition_id, work_id);
CREATE INDEX IF NOT EXISTS catalog_publication_format_kind
    ON catalog_publication_format(book_format, source_id, generation, edition_id, work_id);
CREATE INDEX IF NOT EXISTS catalog_publication_inventory
    ON catalog_publication(source_id, generation, book_format, importable);
CREATE INDEX IF NOT EXISTS catalog_publication_work
    ON catalog_publication(source_id, generation, work_id, importable, gutenberg_has_images, edition_id);
CREATE INDEX IF NOT EXISTS catalog_publication_acquisition
    ON catalog_publication(source_id, generation, acquisition_url);
CREATE INDEX IF NOT EXISTS catalog_publication_subject_path
    ON catalog_publication_subject_assignment(system_id, subject_path, source_id, generation, edition_id, work_id);
CREATE INDEX IF NOT EXISTS catalog_subject_node_parent
    ON catalog_subject_node(system_id, parent_path, title, node_path);
CREATE INDEX IF NOT EXISTS catalog_publication_subject_node_path
    ON catalog_publication_subject_node(system_id, node_path, exact_assignment, source_id, generation, edition_id, work_id);
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

impl From<serde_json::Error> for CatalogCacheError {
    fn from(error: serde_json::Error) -> Self {
        Self::new(error.to_string())
    }
}

#[derive(Clone)]
pub struct CatalogCache {
    pool: CatalogPool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedCatalogPage {
    pub page: CatalogPage,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub provider_paginated: bool,
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
    pub search_index_ready: bool,
    pub active_page_count: u64,
    pub active_publication_count: u64,
    pub active_epub_count: u64,
    pub epub_extractions: CatalogEpubExtractionInventory,
    pub cover_downloads: CatalogCoverDownloadInventory,
    pub metadata_backfill: CatalogMetadataBackfillInventory,
    pub description_backfill: CatalogDescriptionBackfillInventory,
    pub identity_reviews: CatalogIdentityReviewInventory,
    pub sources: Vec<CatalogSourceInventory>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogCoverDownloadInventory {
    pub pending: u64,
    pub running: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub attempts: u64,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogCoverDownloadJob {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub image_url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogMetadataBackfillInventory {
    pub pending: u64,
    pub running: u64,
    pub succeeded: u64,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CatalogDescriptionBackfillInventory {
    pub pending: u64,
    pub running: u64,
    pub succeeded: u64,
    pub no_description: u64,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CatalogIdentityReviewInventory {
    pub ambiguous: u64,
    pub no_match: u64,
    pub insufficient_metadata: u64,
    pub failed: u64,
    pub resolved: u64,
    pub rejected: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CatalogIdentityReview {
    pub review_id: String,
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub title: String,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub publication_year: Option<i32>,
    pub status: String,
    pub detail: Option<String>,
    pub candidates: Vec<CatalogIdentityReviewCandidate>,
    pub candidates_truncated: bool,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CatalogIdentityReviewCandidate {
    pub isbn13: String,
    pub edition_id: String,
    pub work_id: Option<String>,
    pub title: String,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub publication_year: Option<i32>,
    pub matched_publisher: bool,
    pub matched_publication_year: bool,
    pub classifications: Vec<String>,
}

impl From<metadata_contract::EditionIdentityMatch> for CatalogIdentityReviewCandidate {
    fn from(candidate: metadata_contract::EditionIdentityMatch) -> Self {
        let classifications = candidate
            .classifications
            .iter()
            .flat_map(|classification| match classification.scheme {
                metadata_contract::ClassificationScheme::LibraryOfCongress => subject_projection::unified_lcc_subject_paths(&classification.notation),
                metadata_contract::ClassificationScheme::DeweyDecimal => subject_projection::unified_ddc_subject_paths(&classification.notation),
                metadata_contract::ClassificationScheme::Bisac => subject_projection::unified_subject_paths(subject_projection::BISAC_SYSTEM_ID, &classification.notation),
            })
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        Self {
            isbn13: candidate.canonical_isbn13,
            edition_id: candidate.open_library_edition_id,
            work_id: candidate.open_library_work_id,
            title: candidate.title,
            authors: candidate.authors,
            publishers: candidate.publishers,
            publication_year: candidate.publication_year,
            matched_publisher: candidate.matched_publisher,
            matched_publication_year: candidate.matched_publication_year,
            classifications,
        }
    }
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
pub struct CatalogMetadataIndex {
    pub title: String,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub isbns: Vec<String>,
    pub publication_year: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogMetadataBackfillIndexedJob {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub acquisition_url: String,
    pub index: CatalogMetadataIndex,
}

#[derive(Clone, Debug)]
pub struct CatalogMetadataBackfillIndexedUpdate {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub acquisition_url: String,
    pub subjects: Vec<PublicationSubject>,
    pub popularity: Option<CatalogPublicationPopularity>,
    pub index: CatalogMetadataIndex,
    pub identity_result: Option<metadata_contract::EditionIdentityResult>,
}

#[derive(Clone, Debug)]
pub struct CatalogDescriptionBackfillJob {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub acquisition_url: String,
    pub isbns: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CatalogDescriptionBackfillUpdate {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub acquisition_url: String,
    pub description: Option<String>,
    pub subjects: Vec<PublicationSubject>,
    pub open_library_description_missing: bool,
}

pub struct CatalogMetadataBackfillWriteTimings {
    pub subjects_ms: u64,
    pub authors_ms: u64,
    pub identity_review_ms: u64,
    pub index_ms: u64,
    pub status_ms: u64,
    pub transaction_ms: u64,
    pub commit_ms: u64,
}

pub type CatalogSubjectAssignment = subject_projection::ProjectedSubjectAssignment;

pub fn prepare_catalog_subject_assignments(subjects: &[PublicationSubject]) -> Vec<CatalogSubjectAssignment> {
    if subjects.is_empty() {
        return Vec::new();
    }
    subject_projection::project_subjects(subjects).into_iter().chain(subject_projection::project_unified_subjects(subjects)).collect()
}

#[derive(Clone, Debug)]
pub struct CatalogMetadataBackfillUpdate {
    pub source_id: CatalogSourceId,
    pub publication: CatalogPublicationIdentity,
    pub acquisition_url: String,
    pub subjects: Vec<PublicationSubject>,
    pub popularity: Option<CatalogPublicationPopularity>,
    pub metadata: CatalogEpubMetadata,
    pub identity_result: Option<metadata_contract::EditionIdentityResult>,
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct CatalogSubjectProjectionAudit {
    pub dc_subject_occurrences: u64,
    pub dc_subject_occurrences_with_bisac_assignment: u64,
    pub dc_subject_occurrences_without_bisac_assignment: u64,
    pub distinct_dc_subject_headings: u64,
    pub distinct_dc_subject_headings_without_bisac_assignment: u64,
    pub bisac_book_subject_combinations: u64,
    pub bisac_books: u64,
    pub unmatched_dc_subjects: Vec<CatalogUnmatchedDcSubject>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CatalogUnmatchedDcSubject {
    pub subject: String,
    pub occurrences: u64,
    pub examples: Vec<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogCrawlGeneration {
    pub source_id: CatalogSourceId,
    pub generation: u64,
}

#[derive(Clone, Debug)]
pub struct CatalogPageWrite {
    pub page: CatalogPage,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub provider_paginated: bool,
}

#[derive(Clone, Debug)]
pub struct CachedCatalogSubjectBrowseResponse {
    pub response: CatalogSubjectBrowseResponse,
    pub epoch: String,
    pub entry_ids: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CachedCatalogSubjectCollectionState {
    pub epoch: String,
    pub revision: u64,
    pub checked_at: u64,
}

/// An owned crawl-page batch whose wire payloads, hashes, and normalized
/// publication metadata have already been computed outside the writer queue.
pub struct PreparedCatalogPageWrite {
    write: CatalogPageWrite,
    payload: Vec<u8>,
    publications: Vec<PreparedCatalogPublication>,
}

mod cover_queue;
mod crawl_generation;
mod extraction_queue;
mod identity_review;
mod metadata_backfill;
mod pages;
mod process_control;
mod publications;
mod schema;
mod sources;
mod subject_browse;
mod subject_response_cache;
mod subjects;
mod work_supplement_backfill;

pub(crate) use cover_queue::queue_publication_cover;
pub(crate) use extraction_queue::{
    insert_staged_publication, merge_catalog_publication_metadata, merge_catalog_subject_evidence, merge_extracted_author_names, merge_extracted_publication_authors, metadata_index_from_metadata, queue_publication_extraction,
    upsert_active_prepared_publication, upsert_active_publication, upsert_publication_with_sequence,
};
pub(crate) use identity_review::{identity_review_inventory_in, record_identity_review, record_identity_review_failure};
pub(crate) use pages::{
    allocate_sequence, decode_cached, decode_page, optional_stored_u64, project_provider_popularity, publication_key, put_page_row, sequence_in, sequenced_publication_in, sql_decode_error, storage_i64, stored_u64, unique_page_publications,
};
pub use process_control::CatalogProcessSettings;
pub(crate) use publications::{
    book_format_storage, catalog_book_format_from_storage, copy_popularity_metadata, copy_subject_metadata, copy_toc_metadata, gutenberg_edition_has_images, publication_entry_id, publication_extraction, publication_inventory,
    replace_publication_metadata, PreparedCatalogPublication,
};
pub(crate) use schema::{advance_catalog_transport_revision, bounded_crawl_error, unix_timestamp};
pub(crate) use sources::{active_generation_in, active_generation_optional, ensure_source_in, next_sequence_in, refresh_active_counts, source_state_connection, source_state_in};
#[cfg(test)]
pub(crate) use subjects::insert_prepared_subject_assignments;
pub(crate) use subjects::{rebuild_subject_assignments, replace_feed_publication_subjects_prepared, replace_publication_subjects, replace_publication_subjects_prepared, replace_publication_toc, ENRICHED_SUBJECT_POSITION_BASE};

#[cfg(test)]
mod tests;
