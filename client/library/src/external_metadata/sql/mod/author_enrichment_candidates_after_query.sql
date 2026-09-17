WITH lookup_isbn AS (
    SELECT book_row_id,canonical_value,source,'isbn' AS scheme FROM book_identifier identifier
    WHERE scheme='isbn' AND canonical_value IS NOT NULL
      AND NOT EXISTS(SELECT 1 FROM book_value evidence
                     WHERE evidence.book_row_id=identifier.book_row_id AND evidence.kind='enrichment_isbn')
    UNION
    SELECT book_row_id,value,'inspection-reference','isbn' FROM book_value WHERE kind='enrichment_isbn'
) SELECT book.content_hash, GROUP_CONCAT(DISTINCT identifier.canonical_value)
         FROM book
         JOIN lookup_isbn identifier ON identifier.book_row_id=book.row_id
         WHERE (?5 IS NULL OR book.content_hash IN (SELECT value FROM json_each(?5))) AND book.deleted_at IS NULL AND book.content_hash>COALESCE(?4,'')
           AND book.hidden_at IS NULL
               AND NOT EXISTS(SELECT 1 FROM pending_audible_enrichment audible WHERE audible.content_hash=book.content_hash)
           AND identifier.scheme='isbn'
           AND identifier.canonical_value IS NOT NULL
           AND NOT EXISTS(
               SELECT 1 FROM external_metadata_attempt attempt
               WHERE attempt.book_row_id=book.row_id
                 AND attempt.provider_id=?1
                 AND NOT EXISTS(
                     SELECT 1 FROM lookup_isbn current_isbn
                     WHERE current_isbn.book_row_id=book.row_id AND current_isbn.scheme='isbn'
                       AND current_isbn.canonical_value IS NOT NULL
                       AND instr(','||attempt.identifier||',', ','||current_isbn.canonical_value||',')=0
                 )
                 AND attempt.attempted_at >= CAST(strftime('%s','now') AS INTEGER)*1000-2592000000
                 AND (?2=0 OR attempt.status <> 'failed')
           )
         GROUP BY book.row_id
         ORDER BY book.content_hash
         LIMIT ?3
