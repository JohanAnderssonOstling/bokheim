INSERT INTO book(content_hash, title, added_at, format) VALUES
                 (printf('%064x',1), 'Local book', 1, 'epub'),
                 (printf('%064x',2), 'Remote book', 2, 'epub');
             INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id) VALUES
                 ('reading_position',printf('%064x',1),'',1,0,'test',1,'recent-1'),
                 ('reading_position',printf('%064x',2),'',2,0,'test',2,'recent-2');
             INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES
                 ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'local.epub', 'h1', 1),
                 ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'remote.epub', 'h2', 0);