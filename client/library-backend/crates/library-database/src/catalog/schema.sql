CREATE TABLE catalog_browse_revision(
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL,
 release_id INTEGER NOT NULL,format_version INTEGER NOT NULL,taxonomy_revision TEXT NOT NULL
) STRICT;
-- Titles and URLs stay in the source book tables; description text is shared.
-- Only search keys and ranking live here; authors have normalized relationships.
CREATE TABLE catalog_browse_book(
 row_id INTEGER PRIMARY KEY,key BLOB NOT NULL UNIQUE CHECK(length(key)=32),external INTEGER NOT NULL,
 format INTEGER NOT NULL,duration_ms INTEGER,search_title TEXT NOT NULL,search_author TEXT NOT NULL,
 source_priority INTEGER NOT NULL,popularity_score REAL
) STRICT;
CREATE TABLE catalog_browse_language(book_row_id INTEGER NOT NULL,language TEXT NOT NULL,PRIMARY KEY(book_row_id,language)) STRICT, WITHOUT ROWID;
CREATE TABLE catalog_browse_concept(book_row_id INTEGER NOT NULL,concept_id INTEGER NOT NULL,PRIMARY KEY(book_row_id,concept_id)) STRICT, WITHOUT ROWID;
