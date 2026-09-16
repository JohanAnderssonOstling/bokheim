SELECT changed_at,conflict_rank,replica_id,replica_seq,mutation_id FROM sync_state_version
        WHERE state_kind=?1 AND state_key=?2 AND state_subkey=?3