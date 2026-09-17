UPDATE book SET book_metadata=?2,
             description_scanned=CASE WHEN ?3 IS NULL THEN description_scanned ELSE 1 END
             WHERE content_hash=?1 AND (book_metadata IS NOT ?2 OR
                (?3 IS NOT NULL AND description_scanned IS NOT 1))