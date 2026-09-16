INSERT INTO dir(id, parent_id, name) VALUES
                 ('61000000-0000-0000-0000-0000e40c292c', '00000000-0000-0000-0000-000000000000', 'Zulu'),
                 ('62000000-0000-0000-0000-0000e70c2de5', '00000000-0000-0000-0000-000000000000', 'alpha');
             INSERT INTO book(content_hash, title, format) VALUES
                 (printf('%064x',1), 'Zulu Query', 'epub'),
                 (printf('%064x',2), 'Alpha Story', 'epub');
             INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES
                 ('61000000-0000-0000-0000-0000e40c292c', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'zulu.epub', 'zulu'),
                 ('62000000-0000-0000-0000-0000e70c2de5', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'alpha.epub', 'alpha'),
                 ('61000000-0000-0000-0000-0000e40c292c', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'alpha-again.epub', 'alpha-again');