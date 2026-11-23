-- Sync outbox snapshots and acknowledgements.
--
-- The orchestrator reads a publishable snapshot, performs its network work
-- with no database reference held, then commits acknowledgements back. One
-- method call is one transaction; callers batch large id sets themselves.

-- name: sync_outbox_watermark ->
SELECT COALESCE(MAX(replica_seq), 0) FROM sync_outbox;

-- name: sync_acknowledge_mutations!
-- param: ids: &str
DELETE FROM sync_outbox WHERE replica_seq IN (SELECT value FROM json_each(:ids));

-- name: outbox_pending_snapshots ?
SELECT state_kind,state_key,state_subkey,replica_seq FROM sync_outbox WHERE body IS NULL
AND NOT EXISTS(SELECT 1 FROM sync_mutation_issue issue WHERE issue.publisher_seq=sync_outbox.replica_seq AND issue.issue_class=2)
ORDER BY replica_seq;

-- name: outbox_freeze_snapshot !
-- param: body: &SqlParam<'_>
-- param: changed_at: &SqlParam<'_>
-- param: winning_version: &SqlParam<'_>
-- param: replica_seq: &SqlParam<'_>
UPDATE sync_outbox SET body=:body,changed_at=:changed_at,winning_version=:winning_version WHERE replica_seq=:replica_seq;

-- name: outbox_reading_positions ?
SELECT state_key FROM sync_outbox WHERE state_kind='reading_position';
