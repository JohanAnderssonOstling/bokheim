INSERT INTO dir(id, parent_id, name) VALUES
                 ('61000000-0000-0000-0000-0000e40c292c', '00000000-0000-0000-0000-000000000000', 'A'),
                 ('612d6368-696c-6400-0000-000093ae1f6b', '61000000-0000-0000-0000-0000e40c292c', 'Child'),
                 ('62000000-0000-0000-0000-0000e70c2de5', '00000000-0000-0000-0000-000000000000', 'B'),
                 ('656d7074-7900-0000-0000-000018a7beee', '00000000-0000-0000-0000-000000000000', 'Empty'),
                 ('656d7074-792d-6368-696c-6400598db709', '656d7074-7900-0000-0000-000018a7beee', 'Empty child');
             INSERT INTO book(content_hash, title, format) VALUES (printf('%064x',1), 'One', 'epub'), (printf('%064x',2), 'Two', 'pdf'), (printf('%064x',3), 'Three', 'm4b'), (printf('%064x',4), 'Deleted', 'epub');
             INSERT INTO book_language(book_row_id, position, language_tag) VALUES
                 ((SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 0, 'en-US'),
                 ((SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 0, 'sv'),
                 ((SELECT row_id FROM book WHERE content_hash=printf('%064x',3)), 0, 'en');
             UPDATE book SET deleted_at=1 WHERE content_hash=printf('%064x',4);
             INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES
                 ('61000000-0000-0000-0000-0000e40c292c', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one.epub', 'a-one'),
                 ('612d6368-696c-6400-0000-000093ae1f6b', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one-again.epub', 'a-one-again'),
                 ('612d6368-696c-6400-0000-000093ae1f6b', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'two.epub', 'a-two'),
                 ('61000000-0000-0000-0000-0000e40c292c', (SELECT row_id FROM book WHERE content_hash=printf('%064x',4)), 'deleted.epub', 'a-deleted'),
                 ('62000000-0000-0000-0000-0000e70c2de5', (SELECT row_id FROM book WHERE content_hash=printf('%064x',3)), 'three.epub', 'b-three');
             UPDATE book_dir SET is_downloaded=1 WHERE book_row_id IN (SELECT row_id FROM book WHERE content_hash IN (printf('%064x',1), printf('%064x',3)));