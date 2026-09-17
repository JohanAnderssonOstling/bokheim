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
SELECT bokheim_mutation('book_lifecycle',content_hash,2,'',X'',X'',0,'',NULL),2 FROM book WHERE content_hash=:key;

-- name: sync_current_book_deleted?
-- param: key: &str
SELECT bokheim_mutation('book_lifecycle',content_hash,1,'',X'',X'',0,'',trash_origin_dir_id),1 FROM book WHERE content_hash=:key;

-- name: sync_current_book_added?
-- param: key: &str
SELECT bokheim_mutation('book_lifecycle',content_hash,0,COALESCE(title,content_hash),contributors_for_book(row_id),book_metadata,COALESCE(added_at,0),format,NULL),0 FROM book WHERE content_hash=:key;

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
SELECT bokheim_mutation('metadata',content_hash,COALESCE(title,''),contributors_for_book(row_id),book_metadata),0 FROM book WHERE content_hash=:key;

-- name: sync_current_annotation?
-- param: key: &str
SELECT bokheim_mutation('annotation',annotation.id,book.content_hash,annotation.anchor_kind,annotation.anchor_cfi,annotation.anchor_page,annotation.anchor_rects,annotation.anchor_fallback_cfi,annotation.exact_text,annotation.style,annotation.color,annotation.note,annotation.created_at,annotation.modified_at,annotation.deleted_at IS NOT NULL),annotation.deleted_at IS NOT NULL
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

-- name: schema_sync &
-- Durable synchronization outbox, winning versions, and replica metadata.
create table if not exists sync_outbox(
    replica_seq     INTEGER PRIMARY KEY AUTOINCREMENT,
    mutation_id     TEXT not null default (
        lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-' ||
        lower(hex(randomblob(2))) || '-' || lower(hex(randomblob(2))) || '-' ||
        lower(hex(randomblob(6)))
    ) unique,
    state_kind      TEXT not null check(state_kind in (
        'directory_name', 'directory_parent', 'directory_lifecycle',
        'book_lifecycle', 'placement', 'reading_position',
        'annotation', 'metadata', 'description', 'pdf_reader_metadata',
        'book_toc'
    )),
    state_key       TEXT not null,
    state_subkey    TEXT not null default '',
    body            BLOB not null,
    changed_at      INTEGER not null check(changed_at >= 0),
    conflict_rank   INTEGER not null default 0 check(conflict_rank in (0, 1, 2)),
    check(state_subkey is not null),
    unique(state_kind, state_key, state_subkey)
);

-- One durable winning version per independently merged state cell. Domain
-- tables contain materialized values only; all merge ordering lives here.
create table if not exists sync_state_version(
    state_kind      TEXT not null,
    state_key       TEXT not null,
    state_subkey    TEXT not null default '',
    changed_at      INTEGER not null,
    conflict_rank   INTEGER not null check(conflict_rank in (0, 1, 2)),
    replica_id      TEXT not null,
    replica_seq     INTEGER not null,
    mutation_id     TEXT not null,
    primary key(state_kind, state_key, state_subkey)
);

-- Replica metadata lives in one singleton row with one explicit column per
-- value. There is intentionally no key/value pair: every durable value must
-- be declared here so its storage type is visible in the schema.
create table if not exists sync_metadata(
    singleton                   INTEGER primary key check(singleton = 1),
    replica_id                  TEXT,
    change_origin               TEXT not null default 'local' check(change_origin in ('local', 'remote')),
    last_successful_sync_at     INTEGER check(last_successful_sync_at is null or last_successful_sync_at >= 0),
    last_pull_state_seq         INTEGER check(last_pull_state_seq is null or last_pull_state_seq >= 0),
    last_pull_reading_seq       INTEGER check(last_pull_reading_seq is null or last_pull_reading_seq >= 0),
    inventory_repair_kind       TEXT,
    inventory_repair_key        TEXT,
    inventory_repair_subkey     TEXT,
    cursor_recovery_pending     INTEGER check(cursor_recovery_pending is null or cursor_recovery_pending in (0, 1)),
    scan_seq                    INTEGER not null default 0 check(scan_seq >= 0),
    operation_id                TEXT,
    operation_kind              TEXT check(operation_kind is null or operation_kind in ('Import', 'Sync')),
    operation_scan_complete     INTEGER check(operation_scan_complete is null or operation_scan_complete in (0, 1)),
    operation_scan_failed       INTEGER check(operation_scan_failed is null or operation_scan_failed in (0, 1)),
    operation_discovered        INTEGER check(operation_discovered is null or operation_discovered >= 0),
    operation_failures          INTEGER check(operation_failures is null or operation_failures >= 0),
    operation_requires_sync     INTEGER check(operation_requires_sync is null or operation_requires_sync in (0, 1)),
    operation_completed         INTEGER check(operation_completed is null or operation_completed in (0, 1)),
    operation_sync_unfinished   INTEGER check(operation_sync_unfinished is null or operation_sync_unfinished in (0, 1)),
    cloud_storage_enabled       INTEGER check(cloud_storage_enabled is null or cloud_storage_enabled in (0, 1)),
    remote_library_name         TEXT,
    server_library_created      TEXT
);
insert or ignore into sync_metadata(singleton) values(1);

-- Session-local UI history is not synchronization metadata. It has one
-- durable value per library and therefore has its own table.
create table if not exists local_transfer_history(
    singleton    INTEGER primary key check(singleton = 1),
    entries_json TEXT not null
);

-- Assets confirmed to exist remotely. Absence means unknown, not missing.

-- name: cursor_state_select?
SELECT CAST(last_pull_state_seq AS TEXT) FROM sync_metadata WHERE singleton = 1;

-- name: cursor_reading_select?
SELECT CAST(last_pull_reading_seq AS TEXT) FROM sync_metadata WHERE singleton = 1;

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

-- name: syncmeta_replica_id?
SELECT replica_id FROM sync_metadata WHERE singleton = 1;

-- name: syncmeta_last_successful_sync_at?
SELECT CAST(last_successful_sync_at AS TEXT) FROM sync_metadata WHERE singleton = 1;

-- name: syncmeta_last_pull_state_seq?
SELECT CAST(last_pull_state_seq AS TEXT) FROM sync_metadata WHERE singleton = 1;

-- name: syncmeta_last_pull_reading_seq?
SELECT CAST(last_pull_reading_seq AS TEXT) FROM sync_metadata WHERE singleton = 1;

-- name: syncmeta_cursor_recovery_pending?
SELECT CAST(cursor_recovery_pending AS TEXT) FROM sync_metadata WHERE singleton = 1;

-- name: syncmeta_scan_seq?
SELECT CAST(scan_seq AS TEXT) FROM sync_metadata WHERE singleton = 1;

-- name: syncmeta_cloud_storage_enabled?
SELECT CAST(cloud_storage_enabled AS TEXT) FROM sync_metadata WHERE singleton = 1;

-- name: syncmeta_remote_library_name?
SELECT remote_library_name FROM sync_metadata WHERE singleton = 1;

-- name: syncmeta_server_library_created?
SELECT server_library_created FROM sync_metadata WHERE singleton = 1;
