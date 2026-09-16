INSERT INTO sync_outbox(mutation_id, state_kind, state_key, state_subkey, body, changed_at, conflict_rank)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(state_kind, state_key, state_subkey) DO UPDATE SET
                    replica_seq=excluded.replica_seq, mutation_id=excluded.mutation_id,
                    body=excluded.body, changed_at=excluded.changed_at