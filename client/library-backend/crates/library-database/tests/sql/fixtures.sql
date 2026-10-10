
-- name: fixtures_insert_book !
-- param: arg_1: &SqlParam<'_>
INSERT INTO book(content_hash,title,added_at,format) VALUES (:arg_1,'Book',1,1);

-- name: fixtures_insert_book_language !
-- param: arg_1: &SqlParam<'_>
INSERT INTO book_language(book_row_id,language) VALUES (:arg_1,'en');

-- name: fixtures_insert_book_subject !
-- param: arg_1: &SqlParam<'_>
INSERT INTO book_subject(book_row_id,position,name,source) VALUES (:arg_1,0,'Subject','test');

-- name: fixtures_read_book_row_id ->
-- param: arg_1: &SqlParam<'_>
SELECT row_id FROM book WHERE content_hash=:arg_1;

-- name: fixtures_create_book_subject &
CREATE TEMP TRIGGER reject_purge BEFORE DELETE ON book_subject BEGIN SELECT RAISE(ABORT,'injected purge failure'); END;

-- name: fixtures_drop_TRIGGER &
DROP TRIGGER reject_purge;

-- name: fixtures_create_book &
CREATE TEMP TRIGGER reject_purge BEFORE DELETE ON book BEGIN SELECT RAISE(ABORT,'purge failed'); END;

-- name: fixtures_insert_book_2 !
-- param: arg_1: &SqlParam<'_>
INSERT INTO book(content_hash,title,format,added_at) VALUES(:arg_1,'Book',1,1);

-- name: fixtures_insert_book_3 !
-- param: arg_1: &SqlParam<'_>
INSERT INTO book(row_id,content_hash) VALUES(1,:arg_1);

-- name: fixtures_insert_book_subject_2 !
INSERT INTO book_subject(book_row_id,position,name,source,authority,code) VALUES(1,0,'Unmatched subject','file','unknown-authority','unknown-code');

-- name: fixtures_insert_book_unified_concept !
INSERT INTO book_unified_concept VALUES(1,123456,1);

-- name: fixtures_update_library_local_state_taxonomy_revision !
UPDATE library_local_state SET taxonomy_revision=NULL WHERE singleton=1;

-- name: fixtures_read_sync_outbox_count ->
SELECT count(*) FROM sync_outbox;

-- name: fixtures_read_book_subject_name_source_authority ->
SELECT name,source,authority,code FROM book_subject;

-- name: fixtures_insert_book_4 !
-- param: arg_1: &SqlParam<'_>
INSERT INTO book(row_id,content_hash) VALUES(2,:arg_1);

-- name: fixtures_insert_book_subject_3 !
-- param: arg_1: &SqlParam<'_>
INSERT INTO book_subject(book_row_id,position,name,source) VALUES(2,0,:arg_1,'dc' || ':' || 'subject');

-- name: fixtures_read_book_contributor_b_content_hash_c ?
SELECT b.content_hash,c.position,c.name,c.role FROM book_contributor c JOIN book b ON b.row_id=c.book_row_id ORDER BY b.content_hash,c.position;

-- name: fixtures_create_transaction_probe &
CREATE TABLE transaction_probe(value INTEGER);

-- name: fixtures_insert_transaction_probe !
INSERT INTO transaction_probe VALUES (1);

-- name: fixtures_insert_transaction_probe_2 !
INSERT INTO transaction_probe VALUES (2);

-- name: fixtures_insert_transaction_probe_3 !
INSERT INTO transaction_probe VALUES (3);

-- name: fixtures_insert_book_5 !
-- param: arg_1: &SqlParam<'_>
INSERT INTO book(content_hash,title,format) VALUES(:arg_1,'Book',1);

-- name: fixtures_seed_draft_retry !
-- param: arg_1: &SqlParam<'_>
INSERT INTO local_book_draft(content_hash,body,retry_at) VALUES(:arg_1,X'00',999999)
ON CONFLICT(content_hash) DO UPDATE SET retry_at=999999;
-- name: fixtures_seed_cover_check !
-- param: arg_1: &SqlParam<'_>
INSERT INTO book_transfer(content_hash,cover_checked) VALUES(:arg_1,1)
ON CONFLICT(content_hash) DO UPDATE SET cover_checked=1;

-- name: fixtures_update_library_local_state_enrichment_policy !
UPDATE library_local_state SET enrichment_policy=NULL WHERE singleton=1;

-- name: fixtures_read_book_title_read_pos ->
-- param: arg_1: &SqlParam<'_>
SELECT title,read_pos FROM book WHERE content_hash=:arg_1;

-- name: fixtures_read_book_language_language ?
SELECT language FROM book_language ORDER BY language;

-- name: fixtures_read_dir_id_parent_id_name ?
SELECT id,parent_id,name,deleted_at IS NOT NULL FROM dir ORDER BY id;

-- name: fixtures_create_OF &
CREATE TEMP TRIGGER reject_projection BEFORE UPDATE OF canonical_name ON dir BEGIN SELECT RAISE(ABORT,'projection failure'); END;

-- name: fixtures_read_sync_outbox ->
SELECT COUNT(*) FROM sync_outbox WHERE body IS NULL;

-- name: fixtures_detach_curated_concept &
DETACH curated; ATTACH ':' || 'memory' || ':' AS curated;
            CREATE TABLE curated.concept(concept_id INTEGER PRIMARY KEY,preferred_label TEXT NOT NULL);
            INSERT INTO curated.concept VALUES(1,'Science'),(2,'Physics'),(3,'Quantum'),(4,'Optics');
            CREATE TABLE curated.unified_concept_route(route_id INTEGER PRIMARY KEY,concept_id INTEGER NOT NULL,parent_route_id INTEGER NOT NULL);
            INSERT INTO curated.unified_concept_route VALUES(1,1,0),(2,2,1),(3,3,2),(4,4,2);

-- name: fixtures_insert_book_6 !
-- param: arg_1: &SqlParam<'_>
INSERT INTO book(row_id,content_hash,title,search_title,format) VALUES(:arg_1,unhex(printf('%064x',:arg_1)),printf('Book %d',:arg_1),printf('book %d',:arg_1),1);

-- name: fixtures_insert_book_dir !
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
-- param: arg_3: &SqlParam<'_>
INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES(:arg_1,:arg_2,:arg_3,'',1);

-- name: fixtures_insert_book_unified_concept_2 !
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
INSERT INTO book_unified_concept VALUES(:arg_1,:arg_2,1);

-- name: fixtures_update_curated_concept_preferred_label_fields &
UPDATE curated.concept SET preferred_label='1 / science' WHERE concept_id=1;
            UPDATE curated.concept SET preferred_label='Quantum / fields' WHERE concept_id=3;

-- name: fixtures_insert_book_7 !
-- param: arg_1: &SqlParam<'_>
INSERT INTO book(content_hash) VALUES(:arg_1);

-- name: fixtures_insert_book_transfer !
-- param: arg_1: &SqlParam<'_>
INSERT INTO book_transfer(content_hash,book_download,book_download_at) VALUES(:arg_1,'background',10);

-- name: fixtures_detach_curated_taxonomy_meta &
DETACH curated; ATTACH ':' || 'memory' || ':' AS curated;
            CREATE TABLE curated.taxonomy_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE TABLE curated.concept(concept_id INTEGER PRIMARY KEY,preferred_label TEXT NOT NULL);
            INSERT INTO curated.concept VALUES(1,'Transportation'),(2,'Land Transport'),(3,'Automotive'),(4,'Railroads'),(5,'Marine Transport');
            CREATE TABLE curated.unified_concept_route(route_id INTEGER PRIMARY KEY,concept_id INTEGER NOT NULL,parent_route_id INTEGER NOT NULL);
            INSERT INTO curated.unified_concept_route VALUES(1,1,0),(2,2,1),(3,3,2),(4,4,2),(5,5,1);

-- name: fixtures_insert_book_8 !
-- param: arg_1: &SqlParam<'_>
INSERT INTO book(content_hash,title,format) VALUES(unhex(printf('%064x',:arg_1)),printf('Book %d',:arg_1),1);

-- name: fixtures_insert_book_unified_concept_3 !
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
INSERT INTO book_unified_concept(book_row_id,concept_id,mapper_version) VALUES(:arg_1,:arg_2,1);

-- name: fixtures_delete_book_unified_concept !
DELETE FROM book_unified_concept WHERE book_row_id IN (4,5);

-- name: fixtures_delete_book_unified_concept_2 !
DELETE FROM book_unified_concept WHERE book_row_id=3;

-- name: fixtures_detach_curated_taxonomy_meta_2 &
DETACH curated; ATTACH ':' || 'memory' || ':' AS curated;
            CREATE TABLE curated.taxonomy_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE TABLE curated.concept(concept_id INTEGER PRIMARY KEY,preferred_label TEXT NOT NULL);
            INSERT INTO curated.concept VALUES(1,'Science'),(2,'Physics'),(3,'Quantum'),(4,'Optics');
            CREATE TABLE curated.unified_concept_route(route_id INTEGER PRIMARY KEY,concept_id INTEGER NOT NULL,parent_route_id INTEGER NOT NULL);
            INSERT INTO curated.unified_concept_route VALUES(1,1,0),(2,2,1),(3,3,2),(4,4,2);

-- name: fixtures_delete_book_unified_concept_3 !
DELETE FROM book_unified_concept WHERE book_row_id=7;

-- name: fixtures_detach_curated_concept_2 &
DETACH curated; ATTACH ':' || 'memory' || ':' AS curated;
            CREATE TABLE curated.concept(concept_id INTEGER PRIMARY KEY,preferred_label TEXT NOT NULL);
            INSERT INTO curated.concept VALUES(1,'Science'),(2,'Physics'),(3,'Other'),(4,'Quantum');
            CREATE TABLE curated.unified_concept_route(route_id INTEGER PRIMARY KEY,concept_id INTEGER NOT NULL,parent_route_id INTEGER NOT NULL);
            INSERT INTO curated.unified_concept_route VALUES(1,1,0),(2,2,1),(3,3,0),(4,4,2),(5,4,3);

-- name: fixtures_insert_book_dir_2 !
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
-- param: arg_3: &SqlParam<'_>
-- param: arg_4: &SqlParam<'_>
INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES(:arg_1,:arg_2,:arg_3,'',:arg_4);

-- name: fixtures_insert_book_unified_concept_4 !
-- param: arg_1: &SqlParam<'_>
INSERT INTO book_unified_concept VALUES(:arg_1,4,1);

-- name: fixtures_insert_book_language_2 !
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
INSERT INTO book_language VALUES(:arg_1,:arg_2);

-- name: fixtures_insert_book_unified_concept_5 !
INSERT INTO book_unified_concept VALUES(1,3,1);

-- name: fixtures_update_curated_concept_preferred_label_science !
UPDATE curated.concept SET preferred_label='Renamed science' WHERE concept_id=1;

-- name: fixtures_delete_book_dir !
DELETE FROM book_dir WHERE book_row_id=3;

-- name: fixtures_read_sync_clock_sequence ->
SELECT sequence FROM sync_clock;

-- name: fixtures_insert_dir !
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
-- param: arg_3: &SqlParam<'_>
INSERT INTO dir(id,parent_id,name) VALUES(:arg_1,:arg_2,:arg_3);

-- name: fixtures_insert_book_9 !
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
-- param: arg_3: &SqlParam<'_>
INSERT INTO book(content_hash,title,format,search_title) VALUES(unhex(:arg_1),:arg_2,:arg_3,normalize_search_text(:arg_2));

-- name: fixtures_insert_book_dir_3 !
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
-- param: arg_3: &SqlParam<'_>
INSERT OR IGNORE INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES(:arg_1,:arg_2,:arg_3,'',1);

-- name: fixtures_insert_book_language_3 !
-- param: arg_1: &SqlParam<'_>
INSERT OR IGNORE INTO book_language VALUES(:arg_1,'en');

-- name: fixtures_read_dir_id ?
SELECT id FROM dir;

-- name: fixtures_read_subtree_subtree_id_d ->
-- param: arg_1: &SqlParam<'_>
WITH RECURSIVE subtree(id) AS (SELECT :arg_1 UNION SELECT d.id FROM subtree s JOIN dir d ON d.parent_id=s.id WHERE d.id!=s.id AND d.deleted_at IS NULL) SELECT count(*),coalesce(sum(p.is_downloaded),0) FROM subtree s JOIN book_dir p ON p.dir_id=s.id AND p.deleted_at IS NULL JOIN book b ON b.row_id=p.book_row_id AND b.deleted_at IS NULL;

-- name: fixtures_read_dir_subtree_id_id ?
-- param: arg_1: &SqlParam<'_>
WITH RECURSIVE subtree(id) AS (SELECT id FROM dir WHERE id=:arg_1 AND deleted_at IS NULL UNION SELECT d.id FROM dir d JOIN subtree s ON d.parent_id=s.id WHERE d.id!=d.parent_id AND d.deleted_at IS NULL) SELECT lower(hex(b.content_hash)) FROM book b JOIN book_dir p ON p.book_row_id=b.row_id JOIN subtree s ON s.id=p.dir_id WHERE b.deleted_at IS NULL AND p.deleted_at IS NULL ORDER BY b.content_hash;

-- name: fixtures_read_book_b_format_category ?
SELECT CASE b.format WHEN 2 THEN 2 WHEN 8 THEN 4 ELSE 1 END AS category,COUNT(*) FROM book b WHERE b.deleted_at IS NULL AND EXISTS(SELECT 1 FROM book_dir p WHERE p.book_row_id=b.row_id AND p.deleted_at IS NULL) GROUP BY category ORDER BY category;

-- name: fixtures_read_author_identity_count ->
SELECT count(*) FROM author_identity;

-- name: fixtures_create_book_contributor &
CREATE TRIGGER reject_contributor BEFORE INSERT ON book_contributor BEGIN SELECT RAISE(ABORT,'injected contributor failure'); END;

-- name: fixtures_drop_TRIGGER_2 &
DROP TRIGGER reject_contributor;

-- name: fixtures_read_sync_outbox_state_kind_state_subkey_body ?
SELECT state_kind,state_subkey,body FROM sync_outbox WHERE state_kind='book_add' ORDER BY state_kind,state_subkey;

-- name: fixtures_read_local_file_projection_relative_path ->
-- param: arg_1: &SqlParam<'_>
SELECT relative_path FROM local_file_projection WHERE dir_id=:arg_1;

-- name: fixtures_create_ON &
CREATE TRIGGER reject_projection BEFORE UPDATE ON local_directory_projection BEGIN SELECT RAISE(ABORT,'injected projection failure'); END;

-- name: fixtures_read_book_duration_ms_tracks_json ->
-- param: arg_1: &SqlParam<'_>
SELECT duration_ms, tracks_json FROM book WHERE content_hash=:arg_1;

-- name: fixtures_read_local_file_projection_count ->
-- param: arg_1: &SqlParam<'_>
SELECT count(*) FROM local_file_projection WHERE dir_id=:arg_1;

-- name: fixtures_delete_local_file_work &
DELETE FROM local_file_work; DELETE FROM local_book_work; DELETE FROM local_directory_work;

-- name: fixtures_read_local_file_projection_bd_deleted_at_bd ->
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
SELECT bd.deleted_at IS NULL,bd.is_downloaded,bd.local_hash,(SELECT count(*) FROM local_file_projection p WHERE p.content_hash=b.content_hash AND p.dir_id=bd.dir_id) FROM book_dir bd JOIN book b ON b.row_id=bd.book_row_id WHERE b.content_hash=:arg_1 AND bd.dir_id=:arg_2;

-- name: fixtures_read_book_unified_concept_route_route_id_route ->
SELECT route.route_id,route.parent_route_id FROM book_unified_concept assignment JOIN curated.unified_concept_route route ON route.concept_id=assignment.concept_id WHERE route.route_id!=route.parent_route_id LIMIT 1;

-- name: fixtures_read_curated_unified_concept_paths_path_parent_path ->
SELECT path,parent_path FROM curated.unified_concept_paths WHERE concept_id IN (SELECT concept_id FROM book_unified_concept) AND path!=parent_path AND parent_path!='Subject' LIMIT 1;

-- name: fixtures_update_book_dir_deleted_at !
UPDATE book_dir SET deleted_at=10 WHERE book_row_id=(SELECT row_id FROM book WHERE title='Book 4');

-- name: fixtures_update_book_dir_deleted_at_2 !
UPDATE book_dir SET deleted_at=NULL;

-- name: fixtures_delete_book_unified_concept_4 !
DELETE FROM book_unified_concept;

-- name: fixtures_insert_book_dir_4 !
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
-- param: arg_3: &SqlParam<'_>
INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) SELECT :arg_1,row_id,'copy.epub','',:arg_3 FROM book WHERE content_hash=:arg_2;

-- name: fixtures_update_book_dir_is_downloaded !
UPDATE book_dir SET is_downloaded=0;

-- name: fixtures_update_book_dir_is_downloaded_2 !
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
UPDATE book_dir SET is_downloaded=1 WHERE book_row_id IN (SELECT row_id FROM book WHERE content_hash IN (:arg_1,:arg_2));

-- name: fixtures_update_book_format !
-- param: arg_1: &SqlParam<'_>
UPDATE book SET format=2 WHERE content_hash=:arg_1;

-- name: fixtures_insert_book_language_4 !
-- param: arg_1: &SqlParam<'_>
INSERT OR IGNORE INTO book_language(book_row_id,language) SELECT row_id,'en' FROM book WHERE content_hash!=:arg_1;

-- name: fixtures_delete_book_dir_2 !
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
DELETE FROM book_dir WHERE dir_id=:arg_1 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:arg_2);

-- name: fixtures_update_book_dir_dir_id !
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
-- param: arg_3: &SqlParam<'_>
UPDATE book_dir SET dir_id=:arg_1 WHERE dir_id=:arg_2 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=:arg_3);

-- name: fixtures_update_book_dir_is_downloaded_3 !
-- param: arg_1: &SqlParam<'_>
UPDATE book_dir SET is_downloaded=:arg_1;

-- name: fixtures_update_book_dir_deleted_at_3 !
UPDATE book_dir SET deleted_at=1;

-- name: fixtures_update_dir_name_parent_id !
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
UPDATE dir SET name='Moved',parent_id=:arg_1 WHERE id=:arg_2;

-- name: fixtures_update_book_dir_deleted_at_4 !
-- param: arg_1: &SqlParam<'_>
UPDATE book_dir SET deleted_at=1 WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:arg_1);

-- name: fixtures_update_dir_deleted_at !
-- param: arg_1: &SqlParam<'_>
UPDATE dir SET deleted_at=1 WHERE id=:arg_1;

-- name: fixtures_insert_book_language_5 !
INSERT OR IGNORE INTO book_language(book_row_id,language) SELECT row_id,'en' FROM book;

-- name: fixtures_update_book_dir_is_downloaded_4 !
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
UPDATE book_dir SET is_downloaded=:arg_1 WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=:arg_2);

-- name: fixtures_update_book_read_pos_read_progress !
-- param: arg_1: &SqlParam<'_>
-- param: arg_2: &SqlParam<'_>
-- param: arg_3: &SqlParam<'_>
UPDATE book SET read_pos=:arg_1,read_progress=:arg_2 WHERE content_hash=:arg_3;

-- name: fixtures_read_book_read_pos_read_progress ->
-- param: arg_1: &SqlParam<'_>
SELECT read_pos,read_progress FROM book WHERE content_hash=:arg_1;

-- name: fixtures_update_book_duration_ms !
-- param: arg_1: &SqlParam<'_>
UPDATE book SET duration_ms=12345 WHERE content_hash=:arg_1;

-- name: fixtures_update_book_entry_count !
-- param: arg_1: &SqlParam<'_>
UPDATE book SET entry_count=7 WHERE content_hash=:arg_1;

-- name: fixtures_update_book_duration_ms_2 !
-- param: arg_1: &SqlParam<'_>
UPDATE book SET duration_ms=NULL WHERE content_hash=:arg_1;

-- name: fixtures_read_numbers_numbers_n_n &
WITH RECURSIVE numbers(n) AS (VALUES(1000) UNION ALL SELECT n+1 FROM numbers WHERE n<2999) INSERT INTO book(content_hash,title,format) SELECT printf('%064x',n),'Other book',1 FROM numbers;

-- name: fixtures_insert_book_dir_5 !
-- param: arg_1: &SqlParam<'_>
INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) SELECT :arg_1,row_id,content_hash || '.epub','',1 FROM book WHERE title='Other book';

-- name: fixtures_read_numbers_numbers_n_n_2 !
-- param: arg_1: &SqlParam<'_>
WITH RECURSIVE numbers(n) AS (VALUES(1) UNION ALL SELECT n+1 FROM numbers WHERE n<1000)
        INSERT INTO dir(id,parent_id,name) SELECT printf('10000000-0000-0000-0000-%012x',n),:arg_1,printf('Unrelated %d',n) FROM numbers;

-- name: fixtures_insert_book_language_6 !
INSERT OR IGNORE INTO book_language(book_row_id,language) SELECT row_id,'en' FROM book WHERE title!='Other book';

-- name: fixtures_update_book_deleted_at !
-- param: arg_1: &SqlParam<'_>
UPDATE book SET deleted_at=1 WHERE content_hash=:arg_1;

-- name: fixtures_update_book_deleted_at_2 !
-- param: arg_1: &SqlParam<'_>
UPDATE book SET deleted_at=NULL WHERE content_hash=:arg_1;

-- name: fixtures_delete_dir !
-- param: arg_1: &SqlParam<'_>
DELETE FROM dir WHERE id=:arg_1;

-- name: fixtures_delete_book !
-- param: arg_1: &SqlParam<'_>
DELETE FROM book WHERE content_hash=:arg_1;

-- name: fixtures_read_book_subject_code ?
SELECT code FROM book_subject WHERE code IS NOT NULL;

-- name: fixtures_read_author_identity_stable_id ->
SELECT stable_id FROM author_identity;

-- name: fixtures_read_book_contributor_a_stable_id ->
SELECT a.stable_id FROM book_contributor c JOIN author_identity a ON a.id=c.author_identity_id;

-- name: fixtures_read_book_language ->
SELECT COUNT(*) FROM book_language;

-- name: fixtures_read_book ->
SELECT COUNT(*) FROM book;

-- name: fixtures_read_sync_book_record_count ->
-- param: arg_1: &SqlParam<'_>
SELECT count(*) FROM sync_book_record WHERE content_hash=:arg_1;

-- name: fixtures_read_sync_state_version_count ->
-- param: arg_1: &SqlParam<'_>
SELECT count(*) FROM sync_state_version WHERE book_key=:arg_1 AND state_kind!='book_lifecycle';

-- name: fixtures_read_book_unified_concept_count ->
SELECT count(*) FROM book_unified_concept;

-- name: fixtures_read_library_local_state_count ->
SELECT count(*) FROM library_local_state WHERE taxonomy_revision IS NOT NULL;

-- name: fixtures_read_transaction_probe_value ->
SELECT SUM(value) FROM transaction_probe;

-- name: fixtures_read_transaction_probe ->
SELECT COUNT(*) FROM transaction_probe;

-- name: fixtures_read_enrichment_work ->
SELECT draft.retry_at,t.cover_checked FROM local_book_draft draft JOIN book_transfer t USING(content_hash);

-- name: fixtures_read_dir_name ->
-- param: arg_1: &SqlParam<'_>
SELECT name FROM dir WHERE id=:arg_1;

-- name: fixtures_read_book_unified_concept_concept_id ->
SELECT concept_id FROM book_unified_concept WHERE book_row_id=1;

-- name: fixtures_read_sync_state_version_count_2 ->
SELECT count(*) FROM sync_state_version;

-- name: fixtures_read_book_count ->
SELECT count(*) FROM book;

-- name: fixtures_read_sync_metadata_cloud_storage_enabled ->
SELECT cloud_storage_enabled FROM sync_metadata;

-- name: fixtures_check_main_user_version ->
PRAGMA main.user_version;

-- name: fixtures_read_main_sqlite_schema_count ->
SELECT count(*) FROM main.sqlite_schema;

-- name: fixtures_read_book_dir_count ->
SELECT count(*) FROM book_dir WHERE deleted_at IS NULL;

-- Purge ownership checks across both derived subject tables.
-- name: fixtures_subject_row_count ->
-- param: table_name: &str
-- param: book_id: Option<i64>
SELECT CASE :table_name
    WHEN 'book_language' THEN (SELECT count(*) FROM book_language WHERE :book_id IS NULL OR book_row_id=:book_id)
    WHEN 'book_subject' THEN (SELECT count(*) FROM book_subject WHERE :book_id IS NULL OR book_row_id=:book_id)
END;

-- Successive folder mutations exercise count and facet invalidation.
-- name: fixtures_keep_folder_state &
SELECT 1;
-- name: fixtures_trash_every_third_placement &
UPDATE book_dir SET deleted_at=1 WHERE book_row_id%3=0;
-- name: fixtures_trash_every_seventh_book &
UPDATE book SET deleted_at=1 WHERE row_id%7=0;
-- name: fixtures_reparent_nested_folder &
UPDATE dir SET parent_id='00000000-0000-0000-0000-000000000004' WHERE name='Nested';
-- name: fixtures_trash_target_folder &
UPDATE dir SET deleted_at=1 WHERE name='Target';
-- name: fixtures_restore_folders_and_change_formats &
UPDATE dir SET deleted_at=NULL;
UPDATE book SET format=8 WHERE row_id%2=0;

-- Subject replacement regression checks.
-- name: fixtures_subject_projection_counts ->
-- param: book_id: i64
SELECT (SELECT count(*) FROM book_subject WHERE book_row_id=:book_id),
       (SELECT count(*) FROM book_unified_concept WHERE book_row_id=:book_id),
       (SELECT count(*) FROM book_unified_concept_evidence WHERE book_row_id=:book_id);

-- name: fixtures_first_curated_concept ->
SELECT concept_id FROM curated.unified_concept_paths ORDER BY concept_id LIMIT 1;

-- name: fixtures_subject_projection_concept ->
-- param: book_id: i64
SELECT concept_id FROM book_unified_concept WHERE book_row_id=:book_id;

-- name: fixtures_track_subject_projection_writes &
CREATE TEMP TRIGGER track_subject_projection_writes AFTER INSERT ON book_subject
BEGIN INSERT INTO transaction_probe(value) VALUES(1); END;

-- name: fixtures_ready_book_upload !
-- param: content_hash: &SqlParam<'_>
INSERT INTO book_transfer(content_hash,upload_id,upload_size,upload_prepared) VALUES(:content_hash,1,10,1);

-- A reading update must not rewrite metadata, annotations or placements.
-- name: fixtures_guard_non_reading_projection_writes &
CREATE TEMP TRIGGER guard_book_metadata BEFORE UPDATE OF
    title,subtitle,author,search_title,search_author,format,added_at,
    deleted_at,trash_origin_dir_id,entry_count,toc_json,duration_ms,tracks_json ON book
BEGIN SELECT RAISE(ABORT,'unrelated book fields were updated'); END;
CREATE TEMP TRIGGER guard_subjects BEFORE DELETE ON book_subject
BEGIN SELECT RAISE(ABORT,'subjects were rebuilt'); END;
CREATE TEMP TRIGGER guard_annotations BEFORE INSERT ON annotation
BEGIN SELECT RAISE(ABORT,'annotations were rewritten'); END;
CREATE TEMP TRIGGER guard_placements BEFORE INSERT ON book_dir
BEGIN SELECT RAISE(ABORT,'placements were rewritten'); END;

-- name: fixtures_drop_non_reading_projection_guards &
DROP TRIGGER guard_book_metadata;
DROP TRIGGER guard_subjects;
DROP TRIGGER guard_annotations;
DROP TRIGGER guard_placements;
