INSERT INTO book(content_hash, title, format) VALUES
             (printf('%064x', 1), 'English regional', 'epub'),
             (printf('%064x', 2), 'Multilingual', 'epub'),
             (printf('%064x', 3), 'Hidden', 'epub'),
             (printf('%064x', 4), 'Deleted', 'epub'),
             (printf('%064x', 5), 'Trash only', 'epub');
         UPDATE book SET hidden_at=1 WHERE content_hash=printf('%064x', 3);
         UPDATE book SET deleted_at=1 WHERE content_hash=printf('%064x', 4);
         INSERT INTO book_language(book_row_id, position, language_tag) VALUES
             ((SELECT row_id FROM book WHERE content_hash=printf('%064x', 1)), 0, 'en-US'),
             ((SELECT row_id FROM book WHERE content_hash=printf('%064x', 1)), 1, 'en_GB'),
             ((SELECT row_id FROM book WHERE content_hash=printf('%064x', 2)), 0, 'en'),
             ((SELECT row_id FROM book WHERE content_hash=printf('%064x', 2)), 1, 'sv-SE'),
             ((SELECT row_id FROM book WHERE content_hash=printf('%064x', 3)), 0, 'de'),
             ((SELECT row_id FROM book WHERE content_hash=printf('%064x', 4)), 0, 'fr'),
             ((SELECT row_id FROM book WHERE content_hash=printf('%064x', 5)), 0, 'it');
         INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, deleted_at) VALUES
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x', 1)), 'one.epub', 'one', NULL),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x', 2)), 'two.epub', 'two', NULL),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x', 3)), 'three.epub', 'three', NULL),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x', 4)), 'four.epub', 'four', NULL),
             ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x', 5)), 'five.epub', 'five', 1);