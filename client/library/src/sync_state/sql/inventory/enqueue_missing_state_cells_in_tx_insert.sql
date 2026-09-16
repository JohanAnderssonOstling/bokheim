INSERT INTO sync_outbox(state_kind,state_key,state_subkey,body,changed_at,conflict_rank)
             VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(state_kind,state_key,state_subkey) DO NOTHING