SELECT CAST(requested.key AS INTEGER), c.position, a.stable_id, c.name, c.sort_name, c.role
FROM json_each(?1) requested
JOIN book b ON b.content_hash=requested.value
JOIN book_contributor c ON c.book_row_id=b.row_id
LEFT JOIN author_identity a ON a.id=c.author_identity_id
ORDER BY CAST(requested.key AS INTEGER), c.position;
