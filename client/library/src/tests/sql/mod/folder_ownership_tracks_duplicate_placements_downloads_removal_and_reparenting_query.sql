INSERT INTO dir(id, parent_id, name) VALUES
                 ('61000000-0000-0000-0000-0000e40c292c', '00000000-0000-0000-0000-000000000000', 'A'),
                 ('612d6368-696c-6400-0000-000093ae1f6b', '61000000-0000-0000-0000-0000e40c292c', 'Child'),
                 ('62000000-0000-0000-0000-0000e70c2de5', '00000000-0000-0000-0000-000000000000', 'B');
             INSERT INTO book(content_hash, title, format) VALUES
                 (printf('%064x',1), 'One', 'epub'),
                 (printf('%064x',2), 'Two', 'epub');
             INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES
                 ('61000000-0000-0000-0000-0000e40c292c', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one.epub', 'one-a', 0),
                 ('612d6368-696c-6400-0000-000093ae1f6b', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one-again.epub', 'one-child', 1),
                 ('612d6368-696c-6400-0000-000093ae1f6b', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'two.epub', 'two-child', 0);