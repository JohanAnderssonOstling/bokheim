INSERT INTO local_import_inspection(content_hash,format,version) VALUES(?1,?2,?3)
        ON CONFLICT(content_hash) DO UPDATE SET format=excluded.format,version=excluded.version