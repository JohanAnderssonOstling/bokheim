CREATE TABLE IF NOT EXISTS catalog_source (
    source_id TEXT PRIMARY KEY,
    source_url TEXT NOT NULL,
    epoch TEXT NOT NULL,
    active_generation INTEGER NOT NULL,
    next_sequence INTEGER NOT NULL,
    last_crawl_started_at INTEGER,
    last_crawl_succeeded_at INTEGER,
    crawl_status TEXT NOT NULL DEFAULT 'never',
    last_crawl_error TEXT,
    active_page_count INTEGER NOT NULL DEFAULT 0,
    active_book_count INTEGER NOT NULL DEFAULT 0,
    active_epub_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS catalog_server_schema_metadata (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    schema_version INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS catalog_transport_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    epoch TEXT NOT NULL,
    revision INTEGER NOT NULL DEFAULT 1
);

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

CREATE TABLE IF NOT EXISTS catalog_book_entry (
    entry_id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_id TEXT NOT NULL REFERENCES catalog_source(source_id) ON DELETE CASCADE,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    payload_hash TEXT NOT NULL,
    UNIQUE(source_id, edition_id, work_id, payload_hash)
);

CREATE TABLE IF NOT EXISTS catalog_book (
    source_id TEXT NOT NULL REFERENCES catalog_source(source_id) ON DELETE CASCADE,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    entry_id INTEGER NOT NULL REFERENCES catalog_book_entry(entry_id),
    sequence INTEGER NOT NULL,
    payload_hash TEXT NOT NULL,
    normalized_title TEXT NOT NULL,
    provider_popularity INTEGER NOT NULL DEFAULT 0,
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

CREATE TABLE IF NOT EXISTS catalog_book_author (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    normalized_name TEXT NOT NULL,
    PRIMARY KEY(source_id, generation, edition_id, work_id, position),
    FOREIGN KEY(source_id, generation, edition_id, work_id)
        REFERENCES catalog_book(source_id, generation, edition_id, work_id)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS catalog_book_language (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    language TEXT NOT NULL,
    PRIMARY KEY(source_id, generation, edition_id, work_id, language),
    FOREIGN KEY(source_id, generation, edition_id, work_id)
        REFERENCES catalog_book(source_id, generation, edition_id, work_id)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS catalog_book_format (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    book_format TEXT NOT NULL,
    PRIMARY KEY(source_id, generation, edition_id, work_id, book_format),
    FOREIGN KEY(source_id, generation, edition_id, work_id)
        REFERENCES catalog_book(source_id, generation, edition_id, work_id)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS catalog_book_subject (
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
        REFERENCES catalog_book(source_id, generation, edition_id, work_id)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS catalog_book_subject_assignment (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    system_id TEXT NOT NULL,
    subject_path TEXT NOT NULL,
    matcher_version INTEGER NOT NULL,
    PRIMARY KEY(source_id, generation, edition_id, work_id, system_id, subject_path),
    FOREIGN KEY(source_id, generation, edition_id, work_id)
        REFERENCES catalog_book(source_id, generation, edition_id, work_id)
        ON DELETE CASCADE
);

-- Recovery evidence follows assignment and source lifetimes. The source position
-- is the stored position, including the separate enrichment position range.
CREATE TABLE IF NOT EXISTS catalog_subject_lcc_evidence (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    system_id TEXT NOT NULL,
    subject_path TEXT NOT NULL,
    source_position INTEGER NOT NULL,
    concept_id INTEGER NOT NULL,
    original TEXT NOT NULL,
    details_json TEXT NOT NULL,
    format_version INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY(source_id,generation,edition_id,work_id,system_id,subject_path,source_position,concept_id),
    FOREIGN KEY(source_id,generation,edition_id,work_id,system_id,subject_path)
        REFERENCES catalog_book_subject_assignment(source_id,generation,edition_id,work_id,system_id,subject_path) ON DELETE CASCADE,
    FOREIGN KEY(source_id,generation,edition_id,work_id,source_position)
        REFERENCES catalog_book_subject(source_id,generation,edition_id,work_id,position) ON DELETE CASCADE
);

-- Subject browsing uses a materialized hierarchy rather than deriving parent
-- and child paths from every assignment at request time. A membership row is
-- unique per book and ancestor node, so navigation counts are exact
-- COUNT(*) aggregates without a temporary DISTINCT book set.
CREATE TABLE IF NOT EXISTS catalog_subject_node (
    system_id TEXT NOT NULL,
    node_path TEXT NOT NULL,
    parent_path TEXT NOT NULL,
    title TEXT NOT NULL,
    PRIMARY KEY(system_id, node_path)
);

CREATE TABLE IF NOT EXISTS catalog_book_subject_node (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    system_id TEXT NOT NULL,
    node_path TEXT NOT NULL,
    exact_assignment INTEGER NOT NULL CHECK(exact_assignment IN (0, 1)),
    PRIMARY KEY(source_id, generation, edition_id, work_id, system_id, node_path),
    FOREIGN KEY(source_id, generation, edition_id, work_id)
        REFERENCES catalog_book(source_id, generation, edition_id, work_id)
        ON DELETE CASCADE,
    FOREIGN KEY(system_id, node_path)
        REFERENCES catalog_subject_node(system_id, node_path)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS catalog_subject_browse_index_metadata (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    index_version INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS catalog_book_toc (
    source_id TEXT NOT NULL,
    generation INTEGER NOT NULL,
    edition_id TEXT NOT NULL,
    work_id TEXT NOT NULL,
    position INTEGER NOT NULL,
    normalized_title TEXT NOT NULL,
    PRIMARY KEY(source_id, generation, edition_id, work_id, position),
    FOREIGN KEY(source_id, generation, edition_id, work_id)
        REFERENCES catalog_book(source_id, generation, edition_id, work_id)
        ON DELETE CASCADE
);

-- These external-content FTS tables index the existing normalized search
-- columns without duplicating book payloads. Triggers keep the index
-- correct even if this database is temporarily opened by the rollback binary.
CREATE VIRTUAL TABLE IF NOT EXISTS catalog_book_title_fts USING fts5(
    normalized_title,
    content='catalog_book',
    content_rowid='rowid',
    tokenize='trigram'
);
CREATE VIRTUAL TABLE IF NOT EXISTS catalog_book_author_fts USING fts5(
    normalized_name,
    content='catalog_book_author',
    content_rowid='rowid',
    tokenize='trigram'
);
CREATE VIRTUAL TABLE IF NOT EXISTS catalog_book_toc_fts USING fts5(
    normalized_title,
    content='catalog_book_toc',
    content_rowid='rowid',
    tokenize='trigram'
);
CREATE TABLE IF NOT EXISTS catalog_search_index_metadata (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    index_version INTEGER NOT NULL,
    ready INTEGER NOT NULL CHECK(ready IN (0, 1))
);
INSERT OR IGNORE INTO catalog_search_index_metadata(singleton,index_version,ready) VALUES(1,1,0);

CREATE TRIGGER IF NOT EXISTS catalog_book_title_fts_insert AFTER INSERT ON catalog_book BEGIN
    INSERT INTO catalog_book_title_fts(rowid,normalized_title) VALUES(new.rowid,new.normalized_title);
END;
CREATE TRIGGER IF NOT EXISTS catalog_book_title_fts_delete AFTER DELETE ON catalog_book BEGIN
    INSERT INTO catalog_book_title_fts(catalog_book_title_fts,rowid,normalized_title) VALUES('delete',old.rowid,old.normalized_title);
END;
CREATE TRIGGER IF NOT EXISTS catalog_book_title_fts_update AFTER UPDATE OF normalized_title ON catalog_book BEGIN
    INSERT INTO catalog_book_title_fts(catalog_book_title_fts,rowid,normalized_title) VALUES('delete',old.rowid,old.normalized_title);
    INSERT INTO catalog_book_title_fts(rowid,normalized_title) VALUES(new.rowid,new.normalized_title);
END;

CREATE TRIGGER IF NOT EXISTS catalog_book_author_fts_insert AFTER INSERT ON catalog_book_author BEGIN
    INSERT INTO catalog_book_author_fts(rowid,normalized_name) VALUES(new.rowid,new.normalized_name);
END;
CREATE TRIGGER IF NOT EXISTS catalog_book_author_fts_delete AFTER DELETE ON catalog_book_author BEGIN
    INSERT INTO catalog_book_author_fts(catalog_book_author_fts,rowid,normalized_name) VALUES('delete',old.rowid,old.normalized_name);
END;
CREATE TRIGGER IF NOT EXISTS catalog_book_author_fts_update AFTER UPDATE OF normalized_name ON catalog_book_author BEGIN
    INSERT INTO catalog_book_author_fts(catalog_book_author_fts,rowid,normalized_name) VALUES('delete',old.rowid,old.normalized_name);
    INSERT INTO catalog_book_author_fts(rowid,normalized_name) VALUES(new.rowid,new.normalized_name);
END;

CREATE TRIGGER IF NOT EXISTS catalog_book_toc_fts_insert AFTER INSERT ON catalog_book_toc BEGIN
    INSERT INTO catalog_book_toc_fts(rowid,normalized_title) VALUES(new.rowid,new.normalized_title);
END;
CREATE TRIGGER IF NOT EXISTS catalog_book_toc_fts_delete AFTER DELETE ON catalog_book_toc BEGIN
    INSERT INTO catalog_book_toc_fts(catalog_book_toc_fts,rowid,normalized_title) VALUES('delete',old.rowid,old.normalized_title);
END;
CREATE TRIGGER IF NOT EXISTS catalog_book_toc_fts_update AFTER UPDATE OF normalized_title ON catalog_book_toc BEGIN
    INSERT INTO catalog_book_toc_fts(catalog_book_toc_fts,rowid,normalized_title) VALUES('delete',old.rowid,old.normalized_title);
    INSERT INTO catalog_book_toc_fts(rowid,normalized_title) VALUES(new.rowid,new.normalized_title);
END;

CREATE TABLE IF NOT EXISTS catalog_book_popularity (
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
        REFERENCES catalog_book(source_id, generation, edition_id, work_id)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS catalog_book_title
    ON catalog_book(source_id, generation, normalized_title, edition_id, work_id);
CREATE INDEX IF NOT EXISTS catalog_book_entry_lookup
    ON catalog_book(entry_id, source_id, generation);
CREATE INDEX IF NOT EXISTS catalog_book_author_name
    ON catalog_book_author(source_id, generation, normalized_name, edition_id, work_id);
CREATE INDEX IF NOT EXISTS catalog_book_language_tag
    ON catalog_book_language(source_id, generation, language, edition_id, work_id);
CREATE INDEX IF NOT EXISTS catalog_book_format_kind
    ON catalog_book_format(book_format, source_id, generation, edition_id, work_id);
CREATE INDEX IF NOT EXISTS catalog_book_inventory
    ON catalog_book(source_id, generation, book_format, importable);
CREATE INDEX IF NOT EXISTS catalog_book_acquisition
    ON catalog_book(source_id, generation, acquisition_url);
CREATE INDEX IF NOT EXISTS catalog_book_subject_path
    ON catalog_book_subject_assignment(system_id, subject_path, source_id, generation, edition_id, work_id);
CREATE INDEX IF NOT EXISTS catalog_subject_node_parent
    ON catalog_subject_node(system_id, parent_path, title, node_path);
CREATE INDEX IF NOT EXISTS catalog_book_subject_node_path
    ON catalog_book_subject_node(system_id, node_path, exact_assignment, source_id, generation, edition_id, work_id);
CREATE INDEX IF NOT EXISTS catalog_book_toc_title
    ON catalog_book_toc(source_id, generation, normalized_title, edition_id, work_id);
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
CREATE INDEX IF NOT EXISTS catalog_server_gutenberg_work
    ON catalog_book(source_id,generation,work_id,importable,acquisition_url,edition_id);

CREATE TABLE IF NOT EXISTS catalog_process_control (
    process TEXT PRIMARY KEY CHECK(process IN ('catalogue_crawling','cover_downloads','enrichment')),
    enabled INTEGER NOT NULL CHECK(enabled IN (0, 1))
);
INSERT OR IGNORE INTO catalog_process_control(process,enabled) VALUES
    ('catalogue_crawling',0),
    ('cover_downloads',0),
    ('enrichment',0);

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
    staged_path TEXT,
    staged_metadata_only INTEGER NOT NULL DEFAULT 0,
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
    ocr_status TEXT NOT NULL DEFAULT 'not_needed' CHECK(ocr_status IN ('not_needed', 'pending', 'running', 'succeeded')),
    attempts INTEGER NOT NULL DEFAULT 0,
    last_attempt_at INTEGER,
    last_error TEXT,
    ocr_attempts INTEGER NOT NULL DEFAULT 0,
    ocr_last_attempt_at INTEGER,
    ocr_last_error TEXT,
    PRIMARY KEY(source_id, acquisition_url),
    FOREIGN KEY(source_id, acquisition_url)
        REFERENCES catalog_epub_extraction(source_id, acquisition_url)
        ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS catalog_metadata_backfill_status
    ON catalog_metadata_backfill(status, attempts, source_id, acquisition_url);
CREATE INDEX IF NOT EXISTS catalog_metadata_backfill_ocr_status
    ON catalog_metadata_backfill(ocr_status, source_id, acquisition_url);

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
    book_year INTEGER,
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
-- identity per active canonical book. The ingestion tables above stay
-- authoritative; this projection can always be rebuilt atomically.
CREATE TABLE IF NOT EXISTS catalog_browse_serving_metadata (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    index_version INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    ready INTEGER NOT NULL CHECK(ready IN (0, 1))
);
INSERT OR IGNORE INTO catalog_browse_serving_metadata(singleton,index_version,revision,ready)
    VALUES(1,1,0,0);

CREATE TABLE IF NOT EXISTS catalog_browse_book (
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
CREATE INDEX IF NOT EXISTS catalog_browse_book_source
    ON catalog_browse_book(source_id, entry_id);
CREATE INDEX IF NOT EXISTS catalog_browse_book_title
    ON catalog_browse_book(normalized_title, entry_id);
CREATE INDEX IF NOT EXISTS catalog_browse_book_popularity
    ON catalog_browse_book(
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

CREATE TABLE IF NOT EXISTS catalog_enrichment_cache (
    provider TEXT NOT NULL, isbn TEXT NOT NULL, payload TEXT NOT NULL, expires_at INTEGER NOT NULL,
    PRIMARY KEY(provider,isbn)
);
CREATE INDEX IF NOT EXISTS catalog_enrichment_cache_expiry ON catalog_enrichment_cache(expires_at);
CREATE TABLE IF NOT EXISTS catalog_enrichment_checkpoint (
    source_id TEXT NOT NULL, acquisition_url TEXT NOT NULL, input_key TEXT NOT NULL,
    payload TEXT NOT NULL, updated_at INTEGER NOT NULL,
    PRIMARY KEY(source_id,acquisition_url)
);
