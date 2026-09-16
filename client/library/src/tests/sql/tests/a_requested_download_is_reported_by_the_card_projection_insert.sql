INSERT INTO book(content_hash, title, format) VALUES (printf('%064x',1), 'Queued', 'epub');
             INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES
                 ('00000000-0000-0000-0000-000000000000', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one.epub', 'h1', 0);
             INSERT INTO download_requests(target_kind, target_key, origin, created_at)
             VALUES ('book', printf('%064x',1), 'user_initiated', 0);