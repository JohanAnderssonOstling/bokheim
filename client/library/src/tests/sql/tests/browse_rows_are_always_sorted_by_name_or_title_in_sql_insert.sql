INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id) VALUES
             ('reading_position',printf('%064x',1),'',30,0,'test',1,'browse-read-1'),
             ('reading_position',printf('%064x',2),'',20,0,'test',2,'browse-read-2');