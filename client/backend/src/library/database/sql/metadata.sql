-- Book metadata registers and their relational projections.

-- name: upsert_scanned_book_metadata!
-- param: id: &str
-- param: title: &str
-- param: description: &str
-- param: publication_metadata: &[u8]
-- param: added_at: i64
-- param: format: &str
INSERT INTO book(content_hash,title,description,description_scanned,publication_metadata,added_at,format,deleted_at,purged_at)
VALUES(:id,:title,:description,1,:publication_metadata,:added_at,:format,NULL,NULL)
ON CONFLICT(content_hash) DO UPDATE SET
    title=excluded.title,
    format=excluded.format,
    description=excluded.description,
    description_scanned=1,
    publication_metadata=excluded.publication_metadata,
    -- Metadata backfills must not make an existing book look newly added.
    added_at=COALESCE(book.added_at, excluded.added_at),
    deleted_at=NULL,
    purged_at=NULL;

-- name: clear_book_publishers!
-- param: content_hash: &str
DELETE FROM book_publisher WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);
-- name: clear_book_identifiers!
-- param: content_hash: &str
DELETE FROM book_identifier WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);
-- name: clear_book_languages!
-- param: content_hash: &str
DELETE FROM book_language WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);
-- name: clear_book_subjects!
-- param: content_hash: &str
DELETE FROM book_subject WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);
-- name: clear_book_subject_assignments!
-- param: content_hash: &str
DELETE FROM book_subject_assignment WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);
-- name: clear_book_collections!
-- param: content_hash: &str
DELETE FROM book_collection WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);
-- name: clear_book_publication_dates!
-- param: content_hash: &str
DELETE FROM book_publication_date WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);
-- name: clear_book_publication_values!
-- param: content_hash: &str
DELETE FROM book_publication_value WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:content_hash);

-- name: insert_publisher!
-- param: name: &str
-- param: normalized_name: &str
INSERT OR IGNORE INTO publisher(name,normalized_name) VALUES(:name,:normalized_name);
-- name: publisher_id_by_name?
-- param: normalized_name: &str
SELECT id FROM publisher WHERE normalized_name=:normalized_name;
-- name: author_identity_local_id?
-- param: stable_id: &str
SELECT id FROM author_identity WHERE stable_id=:stable_id;

-- name: insert_book_publisher!
-- param: content_hash: &str
-- param: publisher_id: i64
-- param: agent_identity_id: i64
-- param: position: i64
-- param: credited_name: &str
INSERT INTO book_publisher(book_row_id,publisher_id,agent_identity_id,position,credited_name)
VALUES((SELECT row_id FROM book WHERE content_hash=:content_hash),:publisher_id,:agent_identity_id,:position,:credited_name);

-- name: insert_book_identifier!
-- param: content_hash: &str
-- param: position: i64
-- param: scheme: &str
-- param: value: &str
-- param: canonical_value: Option<&str>
-- param: type_evidence: &[u8]
INSERT INTO book_identifier(book_row_id,position,scheme,value,canonical_value,type_evidence)
VALUES((SELECT row_id FROM book WHERE content_hash=:content_hash),:position,:scheme,:value,:canonical_value,:type_evidence);

-- name: insert_book_language!
-- param: content_hash: &str
-- param: position: i64
-- param: language_tag: &str
INSERT INTO book_language(book_row_id,position,language_tag) VALUES((SELECT row_id FROM book WHERE content_hash=:content_hash),:position,:language_tag);

-- name: insert_book_subject!
-- param: content_hash: &str
-- param: position: i64
-- param: name: &str
-- param: source: &str
-- param: authority: Option<&str>
-- param: code: Option<&str>
INSERT INTO book_subject(book_row_id,position,name,source,authority,code) VALUES((SELECT row_id FROM book WHERE content_hash=:content_hash),:position,:name,:source,:authority,:code);

-- name: insert_book_subject_assignment!
-- param: content_hash: &str
-- param: system_id: &str
-- param: subject_path: &str
-- param: matcher_version: i64
INSERT INTO book_subject_assignment(book_row_id,system_id,subject_path,matcher_version)
VALUES((SELECT row_id FROM book WHERE content_hash=:content_hash),:system_id,:subject_path,:matcher_version);

-- name: insert_book_subject_assignment_evidence!
-- param: content_hash: &str
-- param: system_id: &str
-- param: subject_path: &str
-- param: subject_position: i64
INSERT INTO book_subject_assignment_evidence(book_row_id,system_id,subject_path,subject_position)
VALUES((SELECT row_id FROM book WHERE content_hash=:content_hash),:system_id,:subject_path,:subject_position);

-- name: insert_book_collection!
-- param: content_hash: &str
-- param: position: i64
-- param: name: &str
-- param: collection_type: Option<&str>
-- param: group_position: Option<&str>
INSERT INTO book_collection(book_row_id,position,name,collection_type,group_position)
VALUES((SELECT row_id FROM book WHERE content_hash=:content_hash),:position,:name,:collection_type,:group_position);

-- name: insert_book_publication_date!
-- param: content_hash: &str
-- param: position: i64
-- param: value: &str
-- param: normalized_year: Option<i32>
-- param: normalized_month: Option<i64>
-- param: normalized_day: Option<i64>
-- param: event: Option<&str>
INSERT INTO book_publication_date(book_row_id,position,value,normalized_year,normalized_month,normalized_day,event)
VALUES((SELECT row_id FROM book WHERE content_hash=:content_hash),:position,:value,:normalized_year,:normalized_month,:normalized_day,:event);

-- name: insert_book_publication_value!
-- param: content_hash: &str
-- param: kind: &str
-- param: position: i64
-- param: value: &str
-- param: qualifier: Option<&str>
-- param: secondary_value: Option<&str>
INSERT INTO book_publication_value(book_row_id,kind,position,value,qualifier,secondary_value)
VALUES((SELECT row_id FROM book WHERE content_hash=:content_hash),:kind,:position,:value,:qualifier,:secondary_value);

-- name: update_book_title!
-- param: title: &str
-- param: content_hash: &str
UPDATE book SET title=:title WHERE content_hash=:content_hash;

-- name: update_book_description!
-- param: description: &str
-- param: content_hash: &str
UPDATE book SET description=:description, description_scanned=1 WHERE content_hash=:content_hash;

-- name: update_book_publication_metadata!
-- param: publication_metadata: &[u8]
-- param: content_hash: &str
UPDATE book SET publication_metadata=:publication_metadata WHERE content_hash=:content_hash;
