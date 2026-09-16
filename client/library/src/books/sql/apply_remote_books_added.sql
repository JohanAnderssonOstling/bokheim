INSERT INTO book (content_hash, title, read_pos, read_progress, added_at, format, deleted_at, purged_at)
SELECT json_extract(value,'$[0]'), json_extract(value,'$[1]'), NULL, 0,
       json_extract(value,'$[2]'), json_extract(value,'$[3]'), NULL, NULL
FROM json_each(?1) WHERE true ORDER BY CAST(key AS INTEGER)
ON CONFLICT(content_hash) DO UPDATE SET
    title = COALESCE(book.title, excluded.title),
    added_at = excluded.added_at,
    format = excluded.format,
    deleted_at = NULL,
    purged_at = NULL,
    trash_origin_dir_id = NULL;
