INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
        ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET changed_at=excluded.changed_at,
          conflict_rank=excluded.conflict_rank,replica_id=excluded.replica_id,replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id