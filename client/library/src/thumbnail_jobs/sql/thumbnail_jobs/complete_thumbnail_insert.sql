INSERT INTO local_thumbnail_source(content_hash,origin) VALUES(?1,?2)
        ON CONFLICT(content_hash) DO UPDATE SET origin=excluded.origin