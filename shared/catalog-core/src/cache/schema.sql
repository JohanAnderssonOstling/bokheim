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
INSERT OR IGNORE INTO catalog_cache_metadata(singleton, schema_version) VALUES(1, 30);

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
