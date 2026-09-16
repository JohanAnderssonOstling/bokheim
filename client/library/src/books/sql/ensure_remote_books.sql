INSERT OR IGNORE INTO book(content_hash,added_at)
SELECT json_extract(value,'$[0]'),json_extract(value,'$[1]')
FROM json_each(?1) ORDER BY CAST(key AS INTEGER);
