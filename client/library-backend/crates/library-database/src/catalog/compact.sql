CREATE TABLE catalog_browse_revision(
 singleton INTEGER PRIMARY KEY CHECK(singleton=1),version INTEGER NOT NULL,
 release_id INTEGER NOT NULL,format_version INTEGER NOT NULL,taxonomy_revision TEXT NOT NULL
) STRICT;
-- Preserve stable browse IDs while referencing source records by integer ID.
CREATE TABLE catalog_browse_search(
 row_id INTEGER PRIMARY KEY,book_row_id INTEGER NOT NULL UNIQUE REFERENCES book(row_id),
 search_title TEXT NOT NULL,search_author TEXT NOT NULL,source_priority INTEGER NOT NULL,popularity_score REAL
) STRICT;
CREATE VIEW catalog_browse_book AS
 SELECT browse.row_id,source.row_id AS source_row_id,source.key AS identity,source.key AS key,
 source.external,source.format,browse.search_title,browse.search_author,browse.source_priority,browse.popularity_score
 FROM catalog_browse_search browse JOIN book source ON source.row_id=browse.book_row_id;
CREATE VIEW catalog_browse_author AS SELECT author_id,name FROM author;
CREATE VIEW catalog_browse_author_book AS
 SELECT browse.row_id AS book_row_id,author.author_id FROM book_author credit
 JOIN author ON author.row_id=credit.author_row_id
 JOIN catalog_browse_search browse ON browse.book_row_id=credit.book_row_id;
CREATE VIEW catalog_browse_language AS
 SELECT DISTINCT browse.row_id AS book_row_id,
 lower(CASE WHEN instr(language,'-')>0 THEN substr(language,1,instr(language,'-')-1) ELSE language END) AS language
 FROM book_language credit JOIN catalog_browse_search browse ON browse.book_row_id=credit.book_row_id;
CREATE TABLE catalog_browse_concept(book_row_id INTEGER NOT NULL,concept_id INTEGER NOT NULL,PRIMARY KEY(book_row_id,concept_id)) STRICT, WITHOUT ROWID;

-- Author pages seek memberships by author; the source primary key starts with book.
CREATE INDEX IF NOT EXISTS catalog_author_books ON book_author(author_row_id,book_row_id);
