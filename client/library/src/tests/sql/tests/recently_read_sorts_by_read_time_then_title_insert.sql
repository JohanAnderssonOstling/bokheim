INSERT INTO book(content_hash, title, format, read_progress) VALUES
             (printf('%064x',1), 'Zulu', 'epub', 10),
             (printf('%064x',2), 'Beta', 'epub', 10),
             (printf('%064x',3), 'alpha', 'epub', 10);
         INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'zulu.epub', 'z'),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'beta.epub', 'b'),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',3)), 'alpha.epub', 'a');
         INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id) VALUES
             ('reading_position',printf('%064x',1),'',20,0,'test',1,'read-1'),
             ('reading_position',printf('%064x',2),'',30,0,'test',2,'read-2'),
             ('reading_position',printf('%064x',3),'',20,0,'test',3,'read-3');