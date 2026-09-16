INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'alpha.epub', 'a'),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'beta.epub', 'b'),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',3)), 'gamma.epub', 'g');
         -- Gamma read most recently, then Beta. Alpha has never been opened,
         -- so it loses to both despite sorting first by title.
         INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id) VALUES
             ('reading_position',printf('%064x',3),'',30,0,'test',1,'read-3'),
             ('reading_position',printf('%064x',2),'',20,0,'test',2,'read-2');