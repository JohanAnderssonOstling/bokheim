INSERT INTO book(content_hash, title, read_progress, format) VALUES
                 (printf('%064x',1), 'Downloaded', 42.0, 'epub'),
                 (printf('%064x',2), 'Not downloaded', 0.0, 'epub'),
                 (printf('%064x',3), 'An audiobook', 0.0, 'm4b'),
                 (printf('%064x',4), 'Hidden', 0.0, 'epub');
             INSERT INTO book_description(book_row_id,description) SELECT row_id,'A description' FROM book WHERE content_hash=printf('%064x',1);
             UPDATE book SET hidden_at=1 WHERE content_hash=printf('%064x',4);
             INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES
                 ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one.epub', 'h1', 1),
                 ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'two.epub', 'h2', 0),
                 ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',3)), 'three.M4B', 'h3', 1),
                 ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',4)), 'four.epub', 'h4', 1);
             INSERT INTO audiobook_metadata(content_hash, duration_ms, chapter_count, chapter_source, chapters_json) VALUES (printf('%064x',3), 3600000, 12, 'embedded_m4b', '[]');