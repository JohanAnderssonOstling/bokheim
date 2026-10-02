-- Sync cursor and recovery primitives.
--
-- Each entry is one bounded read or one bounded write. Page- and
-- chunk-sized looping belongs to the orchestrator, never to storage.

-- name: sync_state_inventory_page?
-- param: kind: &str
-- param: key: &str
-- param: subkey: &str
SELECT state_kind,state_key,state_subkey FROM sync_state_version WHERE (state_kind,state_key,state_subkey)>(:kind,:key,:subkey) ORDER BY state_kind,state_key,state_subkey LIMIT 256;

-- name: sync_cursor_recovery_pending ->
SELECT cursor_recovery_pending FROM sync_metadata WHERE singleton = 1;

-- name: sync_begin_cursor_recovery!
UPDATE sync_metadata SET last_pull_lifecycle_seq = NULL, last_pull_field_seq = NULL, cursor_recovery_pending = 1 WHERE singleton = 1;

-- name: sync_clear_cursor_recovery!
UPDATE sync_metadata SET cursor_recovery_pending = NULL WHERE singleton = 1;

-- name: sync_pull_cursor?
SELECT CAST(last_pull_lifecycle_seq AS TEXT), CAST(last_pull_field_seq AS TEXT) FROM sync_metadata WHERE singleton = 1;

-- name: cursor_lifecycle_upsert!
-- param: revision: &str
INSERT INTO sync_metadata(singleton, last_pull_lifecycle_seq) VALUES (1, :revision) ON CONFLICT(singleton) DO UPDATE SET last_pull_lifecycle_seq = excluded.last_pull_lifecycle_seq;

-- name: cursor_lifecycle_clear!
UPDATE sync_metadata SET last_pull_lifecycle_seq = NULL WHERE singleton = 1;

-- name: cursor_field_upsert!
-- param: revision: &str
INSERT INTO sync_metadata(singleton, last_pull_field_seq) VALUES (1, :revision) ON CONFLICT(singleton) DO UPDATE SET last_pull_field_seq = excluded.last_pull_field_seq;

-- name: cursor_field_clear!
UPDATE sync_metadata SET last_pull_field_seq = NULL WHERE singleton = 1;

-- name: syncmeta_cursor_recovery_pending?
SELECT CAST(cursor_recovery_pending AS TEXT) FROM sync_metadata WHERE singleton = 1;

-- name: syncmeta_cloud_storage_enabled?
SELECT CAST(cloud_storage_enabled AS TEXT) FROM sync_metadata WHERE singleton = 1;
