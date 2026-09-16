WITH candidates AS (
            SELECT book.row_id,book.content_hash,work.state,
                (SELECT group_concat(canonical_value,',') FROM (
                    SELECT DISTINCT canonical_value FROM book_identifier
                    WHERE book_row_id=book.row_id AND scheme='isbn' AND canonical_value IS NOT NULL ORDER BY canonical_value
                )) AS isbns
            FROM book JOIN local_thumbnail_work work ON work.content_hash=book.content_hash
            WHERE (?4 IS NULL OR book.content_hash IN (SELECT value FROM json_each(?4))) AND book.content_hash>COALESCE(?3,'') AND (work.state='no_cover' OR (work.state='ready' AND lower(book.format)='m4b')) AND book.deleted_at IS NULL AND book.hidden_at IS NULL
               AND NOT EXISTS(SELECT 1 FROM pending_audible_enrichment audible WHERE audible.content_hash=book.content_hash)
        )
        SELECT candidate.content_hash,candidate.isbns FROM candidates candidate
        LEFT JOIN external_metadata_attempt attempt ON attempt.book_row_id=candidate.row_id AND attempt.provider_id=?1
        WHERE candidate.isbns IS NOT NULL AND (
            attempt.book_row_id IS NULL OR attempt.identifier<>candidate.isbns OR (attempt.status='updated' AND candidate.state='no_cover')
            OR (attempt.status='failed' AND attempt.attempted_at <= CAST(strftime('%s','now') AS INTEGER)*1000-30000)
        )
        ORDER BY candidate.content_hash LIMIT ?2
