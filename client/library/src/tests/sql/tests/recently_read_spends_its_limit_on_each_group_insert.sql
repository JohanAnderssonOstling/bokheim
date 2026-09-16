INSERT INTO book(content_hash, title, format, read_progress) VALUES
             (printf('%064x',1), 'Finished newest', 'epub', 100),
             (printf('%064x',2), 'Finished older', 'epub', 99),
             (printf('%064x',3), 'Reading newest', 'epub', 40),
             (printf('%064x',4), 'Reading older', 'epub', 10);
         INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one.epub', 'h1'),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'two.epub', 'h2'),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',3)), 'three.epub', 'h3'),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',4)), 'four.epub', 'h4');
         INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id) VALUES
             ('reading_position',printf('%064x',1),'',40,0,'test',1,'read-1'),
             ('reading_position',printf('%064x',2),'',30,0,'test',2,'read-2'),
             ('reading_position',printf('%064x',3),'',20,0,'test',3,'read-3'),
             ('reading_position',printf('%064x',4),'',10,0,'test',4,'read-4');