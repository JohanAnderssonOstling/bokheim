INSERT INTO book (content_hash, title, read_pos, read_progress, added_at, format, deleted_at, purged_at)
VALUES (?1, ?2, NULL, 0, ?3, ?4, NULL, NULL)
ON CONFLICT(content_hash) DO UPDATE SET
    title = COALESCE(book.title, excluded.title),
    added_at = excluded.added_at,
    format = excluded.format,
    deleted_at = NULL,
    purged_at = NULL,
    trash_origin_dir_id = NULL;
