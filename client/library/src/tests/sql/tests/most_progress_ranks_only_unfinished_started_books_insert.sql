INSERT INTO book(content_hash, title, format, read_progress) VALUES
             (printf('%064x',1), 'Almost done', 'epub', 97),
             (printf('%064x',2), 'Halfway', 'epub', 50),
             (printf('%064x',3), 'Finished', 'epub', 100),
             (printf('%064x',4), 'Not started', 'epub', 0);
         INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one.epub', 'h1'),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'two.epub', 'h2'),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',3)), 'three.epub', 'h3'),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',4)), 'four.epub', 'h4');