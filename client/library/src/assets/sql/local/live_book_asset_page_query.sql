SELECT b.row_id,b.content_hash,
            upload.id IS NOT NULL AND xb.hash IS NULL,
            rt.hash IS NULL AND xt.hash IS NULL AND COALESCE(t.state='ready',0) AND EXISTS(SELECT 1 FROM local_thumbnail_source source WHERE source.content_hash=b.content_hash AND source.origin='local'),
            rt.hash IS NOT NULL, COALESCE(t.state='pending',0)
        FROM (WITH RECURSIVE live_dirs(id) AS (
    SELECT id FROM dir WHERE id='00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL
    UNION ALL
    SELECT d.id FROM dir d JOIN live_dirs p ON d.parent_id=p.id AND d.id!=p.id WHERE d.deleted_at IS NULL
)
SELECT b.row_id,b.content_hash FROM book b
WHERE b.row_id>?1 AND b.content_hash!='' AND EXISTS (
    SELECT 1 FROM book_dir bd JOIN live_dirs d ON d.id=bd.dir_id
    WHERE bd.book_row_id=b.row_id AND bd.deleted_at IS NULL
) ORDER BY b.row_id LIMIT 32) b
        LEFT JOIN local_book_upload upload ON upload.content_hash=b.content_hash
        LEFT JOIN remote_asset rt ON rt.kind='thumbnail' AND rt.hash=b.content_hash
        LEFT JOIN rejected_asset_upload xb ON xb.kind='book' AND xb.hash=b.content_hash AND xb.rejected_at>unixepoch()-300
        LEFT JOIN rejected_asset_upload xt ON xt.kind='thumbnail' AND xt.hash=b.content_hash AND xt.rejected_at>unixepoch()-300
        LEFT JOIN local_thumbnail_work t ON t.content_hash=b.content_hash
        ORDER BY b.row_id