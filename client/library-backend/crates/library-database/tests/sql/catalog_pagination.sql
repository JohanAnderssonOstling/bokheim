CREATE TABLE snapshot(singleton INTEGER PRIMARY KEY CHECK(singleton=1),format_version INTEGER NOT NULL,release_id INTEGER NOT NULL) STRICT;
CREATE TABLE source(id INTEGER PRIMARY KEY,name TEXT NOT NULL UNIQUE) STRICT;
CREATE TABLE description(row_id INTEGER PRIMARY KEY,text TEXT NOT NULL) STRICT;
CREATE TABLE book(
 row_id INTEGER PRIMARY KEY,key BLOB NOT NULL UNIQUE CHECK(length(key)=32),external INTEGER NOT NULL CHECK(external IN(0,1)),
 title TEXT NOT NULL,subtitle TEXT,description_id INTEGER NOT NULL REFERENCES description(row_id),year INTEGER,file_bytes INTEGER,
 source_id INTEGER REFERENCES source(id),entry_url TEXT,download_url TEXT,format INTEGER NOT NULL CHECK(format IN(1,2,4,8,16))
) STRICT;
CREATE VIEW external_book AS SELECT book.row_id,key AS entry_key,source_id,entry_url,title,subtitle,description.text AS description,year,download_url,format
 FROM book JOIN description ON description.row_id=book.description_id WHERE external=1;
CREATE TABLE author(row_id INTEGER PRIMARY KEY,author_id BLOB NOT NULL UNIQUE CHECK(length(author_id)=16),name TEXT NOT NULL) STRICT;
CREATE TABLE book_author(book_row_id INTEGER NOT NULL REFERENCES book(row_id),author_row_id INTEGER NOT NULL REFERENCES author(row_id),PRIMARY KEY(book_row_id,author_row_id)) STRICT, WITHOUT ROWID;
CREATE TABLE book_language(book_row_id INTEGER NOT NULL REFERENCES book(row_id),language TEXT NOT NULL,PRIMARY KEY(book_row_id,language)) STRICT, WITHOUT ROWID;
CREATE TABLE book_identifier(book_row_id INTEGER NOT NULL REFERENCES book(row_id),scheme INTEGER NOT NULL CHECK(scheme BETWEEN 0 AND 12),scheme_label TEXT NOT NULL,value TEXT NOT NULL,scope INTEGER NOT NULL CHECK(scope IN(0,1,2)),PRIMARY KEY(book_row_id,scheme,scheme_label,value,scope)) STRICT, WITHOUT ROWID;
CREATE TABLE subject_source(id INTEGER PRIMARY KEY,name TEXT NOT NULL) STRICT;
CREATE UNIQUE INDEX subject_source_name ON subject_source(name);
CREATE TABLE subject_authority(id INTEGER PRIMARY KEY,name TEXT NOT NULL) STRICT;
CREATE UNIQUE INDEX subject_authority_name ON subject_authority(name);
CREATE TABLE subject(subject_id INTEGER PRIMARY KEY,id TEXT,name TEXT NOT NULL,sort_name TEXT,source_id INTEGER NOT NULL REFERENCES subject_source(id),authority_id INTEGER REFERENCES subject_authority(id),code TEXT) STRICT;
CREATE UNIQUE INDEX subject_identity ON subject(coalesce(id,''),name,coalesce(sort_name,''),source_id,coalesce(authority_id,0),coalesce(code,''));
CREATE TABLE book_subject(book_row_id INTEGER NOT NULL REFERENCES book(row_id),subject_id INTEGER NOT NULL REFERENCES subject(subject_id),PRIMARY KEY(book_row_id,subject_id)) STRICT, WITHOUT ROWID;
CREATE TABLE subject_projection(singleton INTEGER PRIMARY KEY CHECK(singleton=1),taxonomy_revision TEXT NOT NULL) STRICT;
CREATE TABLE book_concept(book_row_id INTEGER NOT NULL REFERENCES book(row_id),concept_id INTEGER NOT NULL,PRIMARY KEY(book_row_id,concept_id)) STRICT, WITHOUT ROWID;
CREATE TABLE book_source(book_row_id INTEGER NOT NULL REFERENCES book(row_id),source_id INTEGER NOT NULL REFERENCES source(id),PRIMARY KEY(book_row_id,source_id)) STRICT, WITHOUT ROWID;
CREATE TABLE source_entry(
 source_id INTEGER NOT NULL REFERENCES source(id),entry_id TEXT NOT NULL,entry_url TEXT NOT NULL,
 hosted_row_id INTEGER REFERENCES book(row_id),external_row_id INTEGER REFERENCES book(row_id),ranking_source_id INTEGER REFERENCES source(id),
 score REAL,popularity INTEGER CHECK(popularity>=0),epub_popularity INTEGER CHECK(epub_popularity>=0),reading_percentile REAL,epub_percentile REAL,
 PRIMARY KEY(source_id,entry_id)
) STRICT, WITHOUT ROWID;
CREATE TABLE source_entry_source(source_id INTEGER NOT NULL,entry_id TEXT NOT NULL,original_source_id INTEGER NOT NULL REFERENCES source(id),PRIMARY KEY(source_id,entry_id,original_source_id),FOREIGN KEY(source_id,entry_id) REFERENCES source_entry(source_id,entry_id)) STRICT, WITHOUT ROWID;
