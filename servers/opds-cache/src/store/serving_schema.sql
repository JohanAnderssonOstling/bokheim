CREATE TABLE IF NOT EXISTS projection_state (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1), authority_epoch TEXT NOT NULL,
 revision INTEGER NOT NULL DEFAULT 0, ready INTEGER NOT NULL DEFAULT 0,
 incarnation TEXT NOT NULL DEFAULT (lower(hex(randomblob(16))))
);
CREATE TABLE IF NOT EXISTS projection_source (
 source_id TEXT PRIMARY KEY, sequence INTEGER NOT NULL,
 edition_cursor TEXT, work_cursor TEXT, done INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS catalog_browse_serving_metadata (
 singleton INTEGER PRIMARY KEY, index_version INTEGER NOT NULL, revision INTEGER NOT NULL, ready INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS catalog_book (
 entry_id INTEGER PRIMARY KEY, source_id TEXT NOT NULL, generation INTEGER NOT NULL,
 edition_id TEXT NOT NULL, work_id TEXT NOT NULL, payload BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS projection_retired(entry_id INTEGER PRIMARY KEY,retired_at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS projection_retired_time ON projection_retired(retired_at,entry_id);
CREATE TABLE IF NOT EXISTS catalog_browse_book (
 entry_id INTEGER PRIMARY KEY, source_id TEXT NOT NULL, generation INTEGER NOT NULL,
 edition_id TEXT NOT NULL, work_id TEXT NOT NULL, normalized_title TEXT NOT NULL,
 provider_popularity INTEGER NOT NULL, sequence INTEGER NOT NULL,
 reading_log_count INTEGER NOT NULL, rating_count INTEGER NOT NULL, has_language INTEGER NOT NULL,
 UNIQUE(source_id,edition_id,work_id)
);
CREATE INDEX IF NOT EXISTS browse_work ON catalog_browse_book(source_id,work_id);
CREATE INDEX IF NOT EXISTS browse_title ON catalog_browse_book(normalized_title,entry_id);
CREATE INDEX IF NOT EXISTS browse_popularity ON catalog_browse_book(
 (reading_log_count>0 OR rating_count>0) DESC,reading_log_count DESC,rating_count DESC,
 provider_popularity DESC,sequence DESC,normalized_title,entry_id);
CREATE TABLE IF NOT EXISTS catalog_browse_search_document(entry_id INTEGER PRIMARY KEY,search_text TEXT NOT NULL);
CREATE VIRTUAL TABLE IF NOT EXISTS catalog_browse_search_fts USING fts5(
 search_text,content='catalog_browse_search_document',content_rowid='entry_id',tokenize='trigram'
);
CREATE TRIGGER IF NOT EXISTS browse_search_delete AFTER DELETE ON catalog_browse_search_document BEGIN
 INSERT INTO catalog_browse_search_fts(catalog_browse_search_fts,rowid,search_text) VALUES('delete',OLD.entry_id,OLD.search_text);
END;
CREATE TRIGGER IF NOT EXISTS browse_search_insert AFTER INSERT ON catalog_browse_search_document BEGIN
 INSERT INTO catalog_browse_search_fts(rowid,search_text) VALUES(NEW.entry_id,NEW.search_text);
END;
CREATE TABLE IF NOT EXISTS catalog_browse_language(entry_id INTEGER NOT NULL,language TEXT NOT NULL,PRIMARY KEY(entry_id,language));
CREATE INDEX IF NOT EXISTS browse_language ON catalog_browse_language(language,entry_id);
CREATE TABLE IF NOT EXISTS catalog_browse_format(entry_id INTEGER NOT NULL,book_format TEXT NOT NULL,PRIMARY KEY(entry_id,book_format));
CREATE INDEX IF NOT EXISTS browse_format ON catalog_browse_format(book_format,entry_id);
CREATE TABLE IF NOT EXISTS catalog_browse_subject(entry_id INTEGER NOT NULL,system_id TEXT NOT NULL,node_path TEXT NOT NULL,PRIMARY KEY(entry_id,system_id,node_path));
CREATE INDEX IF NOT EXISTS browse_subject ON catalog_browse_subject(system_id,node_path,entry_id);
CREATE TABLE IF NOT EXISTS catalog_subject_node(system_id TEXT NOT NULL,node_path TEXT NOT NULL,parent_path TEXT NOT NULL,title TEXT NOT NULL,PRIMARY KEY(system_id,node_path));
