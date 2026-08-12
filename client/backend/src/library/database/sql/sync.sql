-- Canonical synchronization outbox, winning-version, and replica metadata queries.
-- name: get_pending_mutations?
SELECT mutation_id, state_kind, state_key, body, changed_at, replica_seq,
state_subkey
FROM sync_outbox
ORDER BY replica_seq ASC;

-- name: get_sync_state_version?
-- param: state_kind: &str
-- param: state_key: &str
-- param: state_subkey: &str
SELECT changed_at, conflict_rank, replica_id, replica_seq, mutation_id
FROM sync_state_version
WHERE state_kind = :state_kind AND state_key = :state_key AND state_subkey = :state_subkey;

-- name: upsert_sync_state_version!
-- param: state_kind: &str
-- param: state_key: &str
-- param: state_subkey: &str
-- param: changed_at: i64
-- param: conflict_rank: i64
-- param: replica_id: &str
-- param: replica_seq: i64
-- param: mutation_id: &str
INSERT INTO sync_state_version(state_kind, state_key, state_subkey, changed_at, conflict_rank, replica_id, replica_seq, mutation_id)
VALUES (:state_kind, :state_key, :state_subkey, :changed_at, :conflict_rank, :replica_id, :replica_seq, :mutation_id)
ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
    changed_at=excluded.changed_at,
    conflict_rank=excluded.conflict_rank,
    replica_id=excluded.replica_id,
    replica_seq=excluded.replica_seq,
    mutation_id=excluded.mutation_id;

-- name: get_sync_metadata_value?
-- param: key: &str
SELECT value FROM sync_metadata WHERE key = :key;

-- name: set_sync_metadata!
-- param: key: &str
-- param: value: &str
INSERT INTO sync_metadata (key, value) VALUES (:key, :value)
ON CONFLICT(key) DO UPDATE SET value = excluded.value;

-- name: acknowledge_mutation!
-- param: mutation_id: &str
DELETE FROM sync_outbox WHERE mutation_id=:mutation_id;
