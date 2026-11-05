CREATE TABLE snapshot(singleton INTEGER PRIMARY KEY CHECK(singleton=1),format_version INTEGER NOT NULL,release_id INTEGER NOT NULL) STRICT;
CREATE TABLE source(id INTEGER PRIMARY KEY,name TEXT NOT NULL UNIQUE) STRICT;
-- Keep complete descriptions offline while sharing exact repeated lines.
CREATE TABLE description_record(row_id INTEGER PRIMARY KEY) STRICT;
CREATE TABLE description_line(row_id INTEGER PRIMARY KEY,text TEXT NOT NULL) STRICT;
CREATE TABLE description_line_order(
 description_id INTEGER NOT NULL REFERENCES description_record(row_id) ON DELETE CASCADE,
 position INTEGER NOT NULL CHECK(position>=0),line_id INTEGER NOT NULL REFERENCES description_line(row_id),
 PRIMARY KEY(description_id,position)
) STRICT, WITHOUT ROWID;
-- The correlated lookup reads only the selected description's ordered lines.
CREATE VIEW description AS
SELECT record.row_id,coalesce((
 SELECT group_concat(text,char(10)) FROM (
  SELECT line.text FROM description_line_order item JOIN description_line line ON line.row_id=item.line_id
  WHERE item.description_id=record.row_id ORDER BY item.position
 )
),'') AS text FROM description_record record;
CREATE TABLE book(
 row_id INTEGER PRIMARY KEY,key BLOB NOT NULL UNIQUE CHECK(length(key)=32),external INTEGER NOT NULL CHECK(external IN(0,1)),
 title TEXT NOT NULL,subtitle TEXT,description_id INTEGER NOT NULL REFERENCES description_record(row_id),year INTEGER,file_bytes INTEGER,
 source_id INTEGER REFERENCES source(id),entry_url TEXT,download_url TEXT,format INTEGER NOT NULL CHECK(format IN(1,2,4,8,16))
) STRICT;
CREATE VIEW external_book AS SELECT book.row_id,key AS entry_key,source_id,entry_url,title,subtitle,description.text AS description,year,download_url,format
 FROM book JOIN description ON description.row_id=book.description_id WHERE external=1;
CREATE TABLE author(row_id INTEGER PRIMARY KEY,author_id BLOB NOT NULL UNIQUE CHECK(length(author_id)=16),name TEXT NOT NULL) STRICT;
CREATE TABLE book_author(book_row_id INTEGER NOT NULL REFERENCES book(row_id),author_row_id INTEGER NOT NULL REFERENCES author(row_id),PRIMARY KEY(book_row_id,author_row_id)) STRICT, WITHOUT ROWID;
CREATE TABLE book_language(book_row_id INTEGER NOT NULL REFERENCES book(row_id),language TEXT NOT NULL,PRIMARY KEY(book_row_id,language)) STRICT, WITHOUT ROWID;
CREATE TABLE book_identifier(book_row_id INTEGER NOT NULL REFERENCES book(row_id),scheme INTEGER NOT NULL CHECK(scheme BETWEEN 0 AND 12),scheme_label TEXT NOT NULL,value TEXT NOT NULL,scope INTEGER NOT NULL CHECK(scope IN(0,1,2)),PRIMARY KEY(book_row_id,scheme,scheme_label,value,scope)) STRICT, WITHOUT ROWID;
CREATE TABLE subject_projection(singleton INTEGER PRIMARY KEY CHECK(singleton=1),taxonomy_revision TEXT NOT NULL) STRICT;
CREATE TABLE book_concept(book_row_id INTEGER NOT NULL REFERENCES book(row_id),concept_id INTEGER NOT NULL,PRIMARY KEY(book_row_id,concept_id)) STRICT, WITHOUT ROWID;
CREATE TABLE book_source(book_row_id INTEGER NOT NULL REFERENCES book(row_id),source_id INTEGER NOT NULL REFERENCES source(id),PRIMARY KEY(book_row_id,source_id)) STRICT, WITHOUT ROWID;
CREATE TABLE source_entry(
 source_id INTEGER NOT NULL REFERENCES source(id),entry_id INTEGER NOT NULL CHECK(entry_id>0),
 hosted_row_id INTEGER REFERENCES book(row_id),external_row_id INTEGER REFERENCES book(row_id),ranking_source_id INTEGER REFERENCES source(id),
 score REAL,popularity INTEGER CHECK(popularity>=0),epub_popularity INTEGER CHECK(epub_popularity>=0),reading_percentile REAL,epub_percentile REAL,
 PRIMARY KEY(source_id,entry_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE source_entry_source(source_id INTEGER NOT NULL,entry_id INTEGER NOT NULL,original_source_id INTEGER NOT NULL REFERENCES source(id),PRIMARY KEY(source_id,entry_id,original_source_id),FOREIGN KEY(source_id,entry_id) REFERENCES source_entry(source_id,entry_id)) STRICT, WITHOUT ROWID;

-- Fixture writes keep the same convenient interface as legacy snapshots.
CREATE TRIGGER description_fixture_insert INSTEAD OF INSERT ON description BEGIN
 INSERT INTO description_record(row_id) VALUES(NEW.row_id);
 INSERT INTO description_line(text) VALUES(NEW.text);
 INSERT INTO description_line_order VALUES(NEW.row_id,0,last_insert_rowid());
END;
CREATE TRIGGER description_fixture_delete INSTEAD OF DELETE ON description BEGIN
 DELETE FROM description_line_order WHERE description_id=OLD.row_id;
 DELETE FROM description_record WHERE row_id=OLD.row_id;
END;
