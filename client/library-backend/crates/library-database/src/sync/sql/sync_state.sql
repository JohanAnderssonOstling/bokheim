-- Sync cursor, inventory, and recovery primitives.
--
-- Each entry is one bounded read or one bounded write. Page- and
-- chunk-sized looping belongs to the orchestrator, never to storage.

-- name: sync_state_inventory_page?
-- param: kind: &str
-- param: key: &str
-- param: subkey: &str
SELECT state_kind,state_key,state_subkey FROM sync_state_version WHERE (state_kind,state_key,state_subkey)>(:kind,:key,:subkey) ORDER BY state_kind,state_key,state_subkey LIMIT 256;

-- name: sync_outbox_identity_exists?
-- param: kind: &str
-- param: key: &str
-- param: subkey: &str
SELECT EXISTS(SELECT 1 FROM sync_outbox WHERE state_kind=:kind AND state_key=:key AND state_subkey=:subkey);

-- name: sync_enqueue_outbox_cell!
-- param: kind: &str
-- param: key: &str
-- param: subkey: &str
-- param: body: &[u8]
-- param: changed_at: i64
-- param: conflict_rank: i64
INSERT INTO sync_outbox(state_kind,state_key,state_subkey,body,changed_at,conflict_rank)
             VALUES(:kind,:key,:subkey,:body,:changed_at,:conflict_rank)
             ON CONFLICT(state_kind,state_key,state_subkey) DO NOTHING;

-- name: sync_current_directory_name?
-- param: key: &str
SELECT bokheim_mutation('directory_name',id,name),0 FROM dir WHERE id=:key;

-- name: sync_current_directory_parent?
-- param: key: &str
SELECT bokheim_mutation('directory_parent',id,parent_id),0 FROM dir WHERE id=:key;

-- name: sync_current_directory_lifecycle?
-- param: key: &str
SELECT bokheim_mutation('directory_lifecycle',id,CASE WHEN purged_at IS NOT NULL THEN 2 WHEN deleted_at IS NOT NULL THEN 1 ELSE 0 END),CASE WHEN purged_at IS NOT NULL THEN 2 WHEN deleted_at IS NOT NULL THEN 1 ELSE 0 END FROM dir WHERE id=:key;

-- name: sync_current_book_lifecycle_state?
-- param: key: &str
SELECT CASE WHEN purged_at IS NOT NULL THEN 2 WHEN deleted_at IS NOT NULL THEN 1 ELSE 0 END FROM book WHERE content_hash=:key;

-- name: sync_current_book_purged?
-- param: key: &str
SELECT bokheim_mutation('book_lifecycle',content_hash,2,'',NULL,X'',X'',0,'',NULL),2 FROM book WHERE content_hash=:key;

-- name: sync_current_book_deleted?
-- param: key: &str
SELECT bokheim_mutation('book_lifecycle',content_hash,1,'',NULL,X'',X'',0,'',trash_origin_dir_id),1 FROM book WHERE content_hash=:key;

-- name: sync_current_book_added?
-- param: key: &str
SELECT bokheim_mutation('book_lifecycle',content_hash,0,COALESCE(title,content_hash),subtitle,contributors_for_book(row_id),book_metadata,COALESCE(added_at,0),format,NULL),0 FROM book WHERE content_hash=:key;

-- name: sync_current_placement?
-- param: key: &str
-- param: subkey: &str
SELECT bokheim_mutation('placement',book_dir.dir_id,book.content_hash,book_dir.deleted_at IS NULL,book_dir.trash_origin_dir_id),book_dir.deleted_at IS NOT NULL
                 FROM book_dir JOIN book ON book.row_id=book_dir.book_row_id
                 WHERE book.content_hash=:key AND book_dir.dir_id=:subkey;

-- name: sync_current_reading_position?
-- param: key: &str
SELECT bokheim_mutation('reading_position',content_hash,read_pos,COALESCE(read_progress,0)),0 FROM book WHERE content_hash=:key AND read_pos IS NOT NULL;

-- name: sync_current_description?
-- param: key: &str
SELECT bokheim_mutation('description',content_hash,COALESCE((SELECT description FROM book_description WHERE book_row_id=book.row_id),'')),0 FROM book WHERE content_hash=:key;

-- name: sync_current_metadata?
-- param: key: &str
SELECT bokheim_mutation('metadata',content_hash,COALESCE(title,''),subtitle,contributors_for_book(row_id),book_metadata),0 FROM book WHERE content_hash=:key;

-- name: sync_current_annotation?
-- param: key: &str
SELECT bokheim_mutation('annotation',annotation.id,book.content_hash,annotation.detail,annotation.modified_at,annotation.deleted_at IS NOT NULL),annotation.deleted_at IS NOT NULL
                 FROM annotation JOIN book ON book.row_id=annotation.book_row_id WHERE annotation.id=:key;

-- name: sync_cursor_recovery_pending ->
SELECT cursor_recovery_pending FROM sync_metadata WHERE singleton = 1;

-- name: sync_begin_cursor_recovery!
UPDATE sync_metadata SET last_pull_state_seq = NULL, last_pull_reading_seq = NULL, cursor_recovery_pending = 1 WHERE singleton = 1;

-- name: sync_clear_cursor_recovery!
UPDATE sync_metadata SET cursor_recovery_pending = NULL WHERE singleton = 1;

-- name: sync_pull_cursor?
SELECT CAST(last_pull_state_seq AS TEXT), CAST(last_pull_reading_seq AS TEXT) FROM sync_metadata WHERE singleton = 1;

-- name: sync_inventory_checkpoint?
SELECT inventory_repair_kind, inventory_repair_key, inventory_repair_subkey FROM sync_metadata WHERE singleton = 1;

-- name: sync_save_inventory_checkpoint!
-- param: kind: &str
-- param: key: &str
-- param: subkey: &str
INSERT INTO sync_metadata(singleton, inventory_repair_kind, inventory_repair_key, inventory_repair_subkey) VALUES (1, :kind, :key, :subkey)
            ON CONFLICT(singleton) DO UPDATE SET inventory_repair_kind = excluded.inventory_repair_kind, inventory_repair_key = excluded.inventory_repair_key, inventory_repair_subkey = excluded.inventory_repair_subkey;

-- name: sync_clear_inventory_checkpoint!
UPDATE sync_metadata SET inventory_repair_kind = NULL, inventory_repair_key = NULL, inventory_repair_subkey = NULL WHERE singleton = 1;

-- name: cursor_state_upsert!
-- param: revision: &str
INSERT INTO sync_metadata(singleton, last_pull_state_seq) VALUES (1, :revision) ON CONFLICT(singleton) DO UPDATE SET last_pull_state_seq = excluded.last_pull_state_seq;

-- name: cursor_state_clear!
UPDATE sync_metadata SET last_pull_state_seq = NULL WHERE singleton = 1;

-- name: cursor_reading_upsert!
-- param: revision: &str
INSERT INTO sync_metadata(singleton, last_pull_reading_seq) VALUES (1, :revision) ON CONFLICT(singleton) DO UPDATE SET last_pull_reading_seq = excluded.last_pull_reading_seq;

-- name: cursor_reading_clear!
UPDATE sync_metadata SET last_pull_reading_seq = NULL WHERE singleton = 1;

-- name: syncmeta_cursor_recovery_pending?
SELECT CAST(cursor_recovery_pending AS TEXT) FROM sync_metadata WHERE singleton = 1;

-- name: syncmeta_cloud_storage_enabled?
SELECT CAST(cloud_storage_enabled AS TEXT) FROM sync_metadata WHERE singleton = 1;

-- name: sync_current_book_toc?
-- param: key: &str
SELECT bokheim_mutation('book_toc',toc.content_hash,toc.toc_json,COALESCE(metadata.duration_ms,-1)),0 FROM book_toc toc LEFT JOIN audiobook_metadata metadata ON metadata.content_hash=toc.content_hash WHERE toc.content_hash=:key;

-- name: sync_current_pdf_reader_metadata?
-- param: key: &str
-- param: subkey: &str
SELECT bokheim_mutation('pdf_reader_metadata',content_hash,metadata),0
FROM pdf_reader_metadata WHERE content_hash=:key AND checksum=:subkey;
