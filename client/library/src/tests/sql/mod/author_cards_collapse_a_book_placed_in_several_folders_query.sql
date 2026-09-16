INSERT INTO dir(id, parent_id, name) VALUES ('61000000-0000-0000-0000-0000e40c292c', '00000000-0000-0000-0000-000000000000', 'A'), ('62000000-0000-0000-0000-0000e70c2de5', '00000000-0000-0000-0000-000000000000', 'B');
             INSERT INTO book(content_hash, title, format) VALUES (printf('%064x',1), 'Placed twice', 'epub');
             INSERT INTO author_identity(id, stable_id, preferred_name, normalized_name, created_at) VALUES (1, 'a0a00000-0000-0000-0000-000000000001', 'An Author', 'an author', 0);
             INSERT INTO book_contributor(book_row_id, author_identity_id, position, name, role) VALUES ((SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 1, 0, 'An Author', X'617574');
             INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES
                 ('61000000-0000-0000-0000-0000e40c292c', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one.epub', 'h1', 1),
                 ('62000000-0000-0000-0000-0000e70c2de5', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one-again.epub', 'h2', 1);