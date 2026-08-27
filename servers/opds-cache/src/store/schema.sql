CREATE TABLE IF NOT EXISTS catalog_server_schema_metadata (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    schema_version INTEGER NOT NULL
);
INSERT OR IGNORE INTO catalog_server_schema_metadata(singleton, schema_version) VALUES(1, 2);

CREATE INDEX IF NOT EXISTS catalog_server_gutenberg_work
    ON catalog_publication(source_id,generation,work_id,importable,acquisition_url,edition_id);

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
    PRIMARY KEY(entry_id, system_id, node_path)
);
CREATE INDEX IF NOT EXISTS catalog_browse_subject_node
    ON catalog_browse_subject(system_id, node_path, entry_id);

CREATE INDEX IF NOT EXISTS catalog_epub_extraction_status
    ON catalog_epub_extraction(source_id, status);
CREATE INDEX IF NOT EXISTS catalog_epub_failure_category
    ON catalog_epub_failure(category, retryable, last_failed_at);
