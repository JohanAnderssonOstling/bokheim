CREATE TABLE sync_outbox(
                 replica_seq INTEGER PRIMARY KEY,
                 mutation_id TEXT NOT NULL,
                 state_kind TEXT NOT NULL,
                 state_key TEXT NOT NULL,
                 body BLOB NOT NULL,
                 changed_at INTEGER NOT NULL,
                 state_subkey TEXT NOT NULL,
                 conflict_rank INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO sync_outbox(replica_seq, mutation_id, state_kind, state_key, body, changed_at, state_subkey) VALUES
                 (1, '00000000-0000-4000-8000-000000000001', 'book_lifecycle', '1', x'c1', 10, ''),
                 (2, '00000000-0000-4000-8000-000000000002', 'book_lifecycle', '2', x'c1', 11, '');