WITH RECURSIVE live_dirs(id) AS (
            SELECT id FROM dir WHERE id='00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL
            UNION ALL
            SELECT d.id FROM dir d JOIN live_dirs p ON d.parent_id=p.id AND d.id!=p.id WHERE d.deleted_at IS NULL
        ), live_books AS (
            SELECT b.content_hash,
                EXISTS(SELECT 1 FROM book_dir bd JOIN live_dirs d ON d.id=bd.dir_id
                    WHERE bd.book_row_id=b.row_id AND bd.deleted_at IS NULL AND bd.is_downloaded=1) AS local
            FROM book b WHERE b.content_hash!='' AND EXISTS(
                SELECT 1 FROM book_dir bd JOIN live_dirs d ON d.id=bd.dir_id
                WHERE bd.book_row_id=b.row_id AND bd.deleted_at IS NULL)
        )
        SELECT EXISTS(SELECT 1 FROM download_requests WHERE target_kind='book' AND origin='user_initiated')
        OR EXISTS(
            SELECT 1 FROM live_books b
            LEFT JOIN local_book_upload upload ON upload.content_hash=b.content_hash
            LEFT JOIN remote_asset rt ON rt.kind='thumbnail' AND rt.hash=b.content_hash
            LEFT JOIN rejected_asset_upload xb ON xb.kind='book' AND xb.hash=b.content_hash AND xb.rejected_at>unixepoch()-300
            LEFT JOIN rejected_asset_upload xt ON xt.kind='thumbnail' AND xt.hash=b.content_hash AND xt.rejected_at>unixepoch()-300
            LEFT JOIN local_thumbnail_work t ON t.content_hash=b.content_hash
            WHERE (b.local AND upload.id IS NOT NULL AND xb.hash IS NULL)
                OR (t.state='ready' AND rt.hash IS NULL AND xt.hash IS NULL AND EXISTS(SELECT 1 FROM local_thumbnail_source source WHERE source.content_hash=b.content_hash AND source.origin='local'))
                OR (COALESCE(t.state NOT IN ('ready','pending'),1) AND (NOT b.local OR rt.hash IS NOT NULL))
        )