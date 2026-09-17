WITH RECURSIVE live_dirs(id) AS (
        SELECT id FROM dir WHERE id='00000000-0000-0000-0000-000000000000' AND deleted_at IS NULL
        UNION ALL SELECT d.id FROM dir d JOIN live_dirs p ON d.parent_id=p.id AND d.id!=p.id WHERE d.deleted_at IS NULL
    ), eligible AS (
        SELECT current.content_hash,current.checksum,version.size_bytes,
            row_number() OVER(PARTITION BY current.content_hash ORDER BY current.id DESC) AS priority
        FROM local_book_current current
        JOIN local_book_versions version USING(content_hash,checksum)
        JOIN book b ON b.content_hash=current.content_hash AND b.deleted_at IS NULL
        JOIN book_dir bd ON bd.book_row_id=b.row_id AND bd.dir_id=current.dir_id AND bd.deleted_at IS NULL
        JOIN live_dirs d ON d.id=current.dir_id
        WHERE current.origin='local'
    ) INSERT INTO local_book_upload(content_hash,checksum,size_bytes)
      SELECT content_hash,checksum,size_bytes FROM eligible WHERE priority=1
      ON CONFLICT(content_hash) DO NOTHING;
    INSERT INTO local_book_upload(content_hash,checksum,size_bytes)
      SELECT content_hash,checksum,size_bytes FROM local_book_upload WHERE TRUE
      ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;
    INSERT INTO asset_work(content_hash) SELECT content_hash FROM local_book_upload WHERE TRUE
      ON CONFLICT(content_hash) DO UPDATE SET id=excluded.id;