INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id) VALUES
            ('reading_position',printf('%064x',902),'',20,0,'test',1,'compact-read-1'),
            ('reading_position',printf('%064x',903),'',30,0,'test',2,'compact-read-2');