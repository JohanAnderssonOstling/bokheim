WITH RECURSIVE
page AS MATERIALIZED (
    SELECT w.id,w.content_hash,b.row_id,b.deleted_at
    FROM asset_work w LEFT JOIN book b ON b.content_hash=w.content_hash
    WHERE w.id>?1 AND w.id<=?2 ORDER BY w.id LIMIT 32
),
paths(work_id,id,parent_id,path) AS MATERIALIZED (
    SELECT p.id,d.id,d.parent_id,
        CASE WHEN d.id='00000000-0000-0000-0000-000000000000' THEN bd.file_name ELSE d.name || '/' || bd.file_name END
    -- Keep the bounded page outermost; otherwise SQLite can scan all live
    -- placements before joining back to these 32 candidates.
    FROM page p CROSS JOIN book_dir bd ON bd.book_row_id=p.row_id CROSS JOIN dir d ON d.id=bd.dir_id
    WHERE p.deleted_at IS NULL AND bd.deleted_at IS NULL AND d.deleted_at IS NULL
    UNION ALL
    SELECT a.work_id,d.id,d.parent_id,
        CASE WHEN d.id='00000000-0000-0000-0000-000000000000' THEN a.path ELSE d.name || '/' || a.path END
    FROM paths a JOIN dir d ON d.id=a.parent_id
    WHERE a.id!=a.parent_id AND d.deleted_at IS NULL
),
ancestors(work_id,id) AS MATERIALIZED (
    SELECT p.id,bd.dir_id FROM page p JOIN book_dir bd ON bd.book_row_id=p.row_id
    UNION SELECT p.id,f.dir_id FROM page p JOIN local_file_projection f ON f.content_hash=p.content_hash
    UNION SELECT a.work_id,d.parent_id FROM ancestors a JOIN dir d ON d.id=a.id WHERE d.id!=d.parent_id
)
SELECT p.id,p.content_hash,p.row_id IS NOT NULL AND p.deleted_at IS NULL,
    (SELECT '/' || path FROM paths a WHERE a.work_id=p.id AND a.id='00000000-0000-0000-0000-000000000000' ORDER BY path LIMIT 1),
    upload.id IS NOT NULL AND xb.hash IS NULL,
    rt.hash IS NULL AND xt.hash IS NULL AND COALESCE(t.state='ready',0)
        AND EXISTS(SELECT 1 FROM local_thumbnail_source source WHERE source.content_hash=p.content_hash AND source.origin='local'),
    rt.hash IS NOT NULL,COALESCE(t.state='pending',0),
    EXISTS(SELECT 1 FROM download_requests WHERE target_kind='book' AND target_key=p.content_hash AND origin='user_initiated'),
    xb.hash IS NOT NULL OR xt.hash IS NOT NULL,
    EXISTS(SELECT 1 FROM local_book_work WHERE content_hash=p.content_hash)
        OR EXISTS(SELECT 1 FROM local_file_work WHERE operation IN ('restore','trash') AND content_hash=p.content_hash)
        OR EXISTS(SELECT 1 FROM ancestors a JOIN local_directory_work w ON w.dir_id=a.id WHERE a.work_id=p.id)
FROM page p
LEFT JOIN local_book_upload upload ON upload.content_hash=p.content_hash
LEFT JOIN remote_asset rt ON rt.kind='thumbnail' AND rt.hash=p.content_hash
LEFT JOIN rejected_asset_upload xb ON xb.kind='book' AND xb.hash=p.content_hash AND xb.rejected_at>unixepoch()-300
LEFT JOIN rejected_asset_upload xt ON xt.kind='thumbnail' AND xt.hash=p.content_hash AND xt.rejected_at>unixepoch()-300
LEFT JOIN local_thumbnail_work t ON t.content_hash=p.content_hash
ORDER BY p.id
