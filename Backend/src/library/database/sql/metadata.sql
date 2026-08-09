-- Book metadata registers and their relational projections.

-- name: upsert_scanned_book_metadata!
-- param: id: &str
-- param: title: &str
-- param: description: &str
-- param: publication_metadata: &[u8]
-- param: added_at: i64
-- param: format: &str
INSERT INTO book(id,title,description,description_scanned,publication_metadata,added_at,format,deleted_at)
VALUES(:id,:title,:description,1,:publication_metadata,:added_at,:format,NULL)
ON CONFLICT(id) DO UPDATE SET
    title=excluded.title,
    format=excluded.format,
    description=excluded.description,
    description_scanned=1,
    publication_metadata=excluded.publication_metadata,
    added_at=excluded.added_at,
    deleted_at=NULL;

-- name: clear_book_publishers!
-- param: book_id: &str
DELETE FROM book_publisher WHERE book_id=:book_id;
-- name: clear_book_identifiers!
-- param: book_id: &str
DELETE FROM book_identifier WHERE book_id=:book_id;
-- name: clear_book_languages!
-- param: book_id: &str
DELETE FROM book_language WHERE book_id=:book_id;
-- name: clear_book_subjects!
-- param: book_id: &str
DELETE FROM book_subject WHERE book_id=:book_id;
-- name: clear_book_collections!
-- param: book_id: &str
DELETE FROM book_collection WHERE book_id=:book_id;
-- name: clear_book_publication_dates!
-- param: book_id: &str
DELETE FROM book_publication_date WHERE book_id=:book_id;
-- name: clear_book_publication_values!
-- param: book_id: &str
DELETE FROM book_publication_value WHERE book_id=:book_id;

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
-- param: book_id: &str
-- param: publisher_id: i64
-- param: agent_identity_id: i64
-- param: position: i64
-- param: credited_name: &str
INSERT INTO book_publisher(book_id,publisher_id,agent_identity_id,position,credited_name)
VALUES(:book_id,:publisher_id,:agent_identity_id,:position,:credited_name);

-- name: insert_book_identifier!
-- param: book_id: &str
-- param: position: i64
-- param: scheme: &str
-- param: value: &str
-- param: canonical_value: Option<&str>
-- param: type_evidence: &[u8]
INSERT INTO book_identifier(book_id,position,scheme,value,canonical_value,type_evidence)
VALUES(:book_id,:position,:scheme,:value,:canonical_value,:type_evidence);

-- name: insert_book_language!
-- param: book_id: &str
-- param: position: i64
-- param: language_tag: &str
INSERT INTO book_language(book_id,position,language_tag) VALUES(:book_id,:position,:language_tag);

-- name: insert_book_subject!
-- param: book_id: &str
-- param: position: i64
-- param: name: &str
-- param: authority: Option<&str>
-- param: code: Option<&str>
INSERT INTO book_subject(book_id,position,name,authority,code) VALUES(:book_id,:position,:name,:authority,:code);

-- name: insert_book_collection!
-- param: book_id: &str
-- param: position: i64
-- param: name: &str
-- param: collection_type: Option<&str>
-- param: group_position: Option<&str>
INSERT INTO book_collection(book_id,position,name,collection_type,group_position)
VALUES(:book_id,:position,:name,:collection_type,:group_position);

-- name: insert_book_publication_date!
-- param: book_id: &str
-- param: position: i64
-- param: value: &str
-- param: normalized_year: Option<i32>
-- param: normalized_month: Option<i64>
-- param: normalized_day: Option<i64>
-- param: event: Option<&str>
INSERT INTO book_publication_date(book_id,position,value,normalized_year,normalized_month,normalized_day,event)
VALUES(:book_id,:position,:value,:normalized_year,:normalized_month,:normalized_day,:event);

-- name: insert_book_publication_value!
-- param: book_id: &str
-- param: kind: &str
-- param: position: i64
-- param: value: &str
-- param: qualifier: Option<&str>
-- param: secondary_value: Option<&str>
INSERT INTO book_publication_value(book_id,kind,position,value,qualifier,secondary_value)
VALUES(:book_id,:kind,:position,:value,:qualifier,:secondary_value);

-- name: update_book_title!
-- param: title: &str
-- param: book_id: &str
UPDATE book SET title=:title WHERE id=:book_id;

-- name: update_book_description!
-- param: description: &str
-- param: book_id: &str
UPDATE book SET description=:description, description_scanned=1 WHERE id=:book_id;

-- name: update_book_publication_metadata!
-- param: publication_metadata: &[u8]
-- param: book_id: &str
UPDATE book SET publication_metadata=:publication_metadata WHERE id=:book_id;
