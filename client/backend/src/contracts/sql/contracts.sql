-- Browser-worker contract database probes.

-- name: fixture_book_exists ->
-- param: content_hash: _
SELECT EXISTS(SELECT 1 FROM book WHERE content_hash = :content_hash);

-- name: insert_reading_fixture!
-- param: content_hash: _
INSERT INTO book(content_hash, title, format) VALUES (:content_hash, 'Reading fixture', 'epub');

-- name: clear_sync_outbox!
DELETE FROM sync_outbox;

-- name: asset_work_count ->
SELECT count(*) FROM asset_work;

-- name: clear_asset_work!
DELETE FROM asset_work;

-- name: fixture_book_title ->
-- param: content_hash: _
SELECT title FROM book WHERE content_hash = :content_hash;

-- name: create_old_schema &
CREATE TABLE saved_data(value TEXT);
INSERT INTO saved_data VALUES('keep');
PRAGMA user_version=29;

-- name: old_schema_value ->
SELECT value FROM saved_data;

-- name: old_schema_table_count ->
SELECT COUNT(*) FROM sqlite_schema WHERE type='table';

-- name: probe_database ->
SELECT 1;

-- name: insert_download_fixture!
-- param: content_hash: _
INSERT OR IGNORE INTO book(content_hash, title, format) VALUES (:content_hash, 'Download fixture', 'epub');

-- name: insert_download_placement!
-- param: content_hash: _
INSERT OR IGNORE INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded)
VALUES ('00000000-0000-0000-0000-000000000000',
        (SELECT row_id FROM book WHERE content_hash = :content_hash),
        'download.epub', 'fixture', 0);

-- name: insert_paged_fixture!
-- param: content_hash: _
INSERT INTO book(content_hash, title, format) VALUES (:content_hash, 'Paged fixture', 'epub');

-- name: reading_position_mutation_id ->
-- param: content_hash: _
SELECT mutation_id FROM sync_outbox
WHERE state_kind = 'reading_position' AND state_key = :content_hash;

-- name: local_file_projection_count ->
SELECT COUNT(*) FROM local_file_projection;

-- name: local_file_work_count ->
SELECT COUNT(*) FROM local_file_work;

-- name: live_shelf_id ->
SELECT id FROM dir WHERE name = 'Shelf' AND deleted_at IS NULL;
