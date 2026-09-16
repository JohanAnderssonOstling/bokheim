INSERT INTO external_metadata_attempt(book_row_id,provider_id,identifier,status,detail,attempted_at)
         VALUES((SELECT row_id FROM book WHERE content_hash=?1),?2,?3,?4,?5,CAST(strftime('%s','now') AS INTEGER)*1000)
         ON CONFLICT(book_row_id,provider_id) DO UPDATE SET identifier=excluded.identifier,status=excluded.status,detail=excluded.detail,attempted_at=excluded.attempted_at