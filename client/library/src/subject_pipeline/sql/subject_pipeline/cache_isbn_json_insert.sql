INSERT INTO local_isbn_response VALUES(?1,?2,CAST(strftime('%s','now') AS INTEGER)+2592000)
        ON CONFLICT(isbn) DO UPDATE SET response=excluded.response,expires_at=excluded.expires_at