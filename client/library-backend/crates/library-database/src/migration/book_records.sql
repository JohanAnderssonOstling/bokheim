-- Remove the old book_add exclusion without changing other register values.
CREATE TABLE sync_state_version_new(
    state_kind TEXT NOT NULL,
    state_key BLOB NOT NULL,
    state_subkey TEXT NOT NULL DEFAULT '',
    changed_at INTEGER NOT NULL,
    conflict_rank INTEGER NOT NULL CHECK(conflict_rank IN (0,1,2)),
    replica_id TEXT NOT NULL,
    replica_seq INTEGER NOT NULL,
    body BLOB,
    book_key BLOB,
    PRIMARY KEY(state_kind,state_key,state_subkey)
);
INSERT INTO sync_state_version_new(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,body,book_key)
SELECT state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,body,book_key FROM sync_state_version;
DROP TABLE sync_state_version;
ALTER TABLE sync_state_version_new RENAME TO sync_state_version;
