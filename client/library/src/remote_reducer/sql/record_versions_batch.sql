INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id)
        SELECT json_extract(value,'$[0]'),json_extract(value,'$[1]'),json_extract(value,'$[2]'),
               json_extract(value,'$[3]'),json_extract(value,'$[4]'),json_extract(value,'$[5]'),
               json_extract(value,'$[6]'),json_extract(value,'$[7]')
        FROM json_each(?1) WHERE true ORDER BY CAST(key AS INTEGER)
        ON CONFLICT(state_kind,state_key,state_subkey) DO UPDATE SET changed_at=excluded.changed_at,
          conflict_rank=excluded.conflict_rank,replica_id=excluded.replica_id,replica_seq=excluded.replica_seq,mutation_id=excluded.mutation_id
