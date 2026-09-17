-- Sync outbox snapshots and acknowledgements.
--
-- The orchestrator reads a publishable snapshot, performs its network work
-- with no database reference held, then commits acknowledgements back. One
-- method call is one transaction; callers batch large id sets themselves.

-- name: sync_outbox_rows?
SELECT mutation_id, state_kind, state_key, body, changed_at, replica_seq,
state_subkey
FROM sync_outbox
ORDER BY replica_seq ASC;

-- name: sync_pending_uploads?
SELECT content_hash FROM local_book_upload;

-- name: sync_cloud_storage_enabled ->
SELECT cloud_storage_enabled FROM sync_metadata WHERE singleton = 1;

-- name: sync_acknowledge_mutations!
-- param: ids: &str
DELETE FROM sync_outbox WHERE mutation_id IN (SELECT value FROM json_each(:ids));

-- name: enqueue_outbox_reading_change!
-- param: mutation_id: &str
-- param: state_key: &str
-- param: body: &[u8]
-- param: changed_at: i64
INSERT INTO sync_outbox(mutation_id,state_kind,state_key,body,changed_at) VALUES(:mutation_id,'reading_position',:state_key,:body,:changed_at);

-- name: clear_sync_outbox!
DELETE FROM sync_outbox;

-- name: seed_poison_outbox_mutation!
-- param: replica_seq: i64
-- param: mutation_id: &str
-- param: body: &[u8]
-- param: changed_at: i64
INSERT INTO sync_outbox(replica_seq,mutation_id,state_kind,state_key,body,changed_at) VALUES(:replica_seq,:mutation_id,'reading_position','1',:body,:changed_at);
