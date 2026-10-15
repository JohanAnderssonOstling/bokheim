-- Sync outbox snapshots and acknowledgements.
--
-- The orchestrator reads a publishable snapshot, performs its network work
-- with no database reference held, then commits acknowledgements back. One
-- method call is one transaction; callers batch large id sets themselves.

-- name: sync_outbox_watermark ->
SELECT COALESCE(MAX(replica_seq), 0) FROM sync_outbox;

-- name: sync_pending_uploads?
SELECT content_hash FROM book_transfer WHERE upload_id IS NOT NULL;

-- name: sync_cloud_storage_enabled ->
SELECT cloud_storage_enabled FROM sync_metadata WHERE singleton = 1;

-- name: sync_acknowledge_mutations!
-- param: ids: &str
DELETE FROM sync_outbox WHERE replica_seq IN (SELECT value FROM json_each(:ids));

-- name: clear_sync_outbox!
DELETE FROM sync_outbox;

-- name: seed_poison_outbox_mutation!
-- param: replica_seq: i64
-- param: body: &[u8]
-- param: changed_at: i64
INSERT INTO sync_outbox(replica_seq,state_kind,state_key,body,changed_at) VALUES(:replica_seq,'reading_position','1',:body,:changed_at);


-- name: outbox_pending_snapshots ?
SELECT state_kind,state_key,state_subkey,replica_seq FROM sync_outbox WHERE body IS NULL ORDER BY replica_seq;

-- name: outbox_freeze_snapshot !
-- param: body: &SqlParam<'_>
-- param: changed_at: &SqlParam<'_>
-- param: winning_version: &SqlParam<'_>
-- param: replica_seq: &SqlParam<'_>
UPDATE sync_outbox SET body=:body,changed_at=:changed_at,winning_version=:winning_version WHERE replica_seq=:replica_seq;

-- name: sync_pending_book_drafts ?
SELECT content_hash FROM local_book_draft;

-- name: outbox_reading_positions ?
SELECT state_key FROM sync_outbox WHERE state_kind='reading_position';
