INSERT INTO dir(id, parent_id, name) VALUES ('61000000-0000-0000-0000-0000e40c292c', '00000000-0000-0000-0000-000000000000', 'A');
         INSERT INTO book(content_hash, title, format) VALUES
             (printf('%064x', 1), 'EPUB', 'epub'),
             (printf('%064x', 2), 'MOBI', 'mobi'),
             (printf('%064x', 3), 'PDF', 'pdf'),
             (printf('%064x', 4), 'Audio', 'm4b'),
             (printf('%064x', 5), 'Hidden PDF', 'pdf'),
             (printf('%064x', 6), 'Deleted audio', 'm4b'),
             (printf('%064x', 7), 'Trash-only EPUB', 'epub');
         UPDATE book SET hidden_at=1 WHERE content_hash=printf('%064x', 5);
         UPDATE book SET deleted_at=1 WHERE content_hash=printf('%064x', 6);
         INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, deleted_at) VALUES
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x', 1)), 'one.epub', 'one', NULL),
             ('61000000-0000-0000-0000-0000e40c292c', (SELECT row_id FROM book WHERE content_hash=printf('%064x', 1)), 'one-again.epub', 'one-again', NULL),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x', 2)), 'two.mobi', 'two', NULL),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x', 3)), 'three.pdf', 'three', NULL),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x', 4)), 'four.m4b', 'four', NULL),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x', 5)), 'five.pdf', 'five', NULL),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x', 6)), 'six.m4b', 'six', NULL),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x', 7)), 'seven.epub', 'seven', 1);