WITH lookup_isbn AS (
    SELECT book_row_id,canonical_value,source,'isbn' AS scheme FROM book_identifier identifier
    WHERE scheme='isbn' AND canonical_value IS NOT NULL
      AND NOT EXISTS(SELECT 1 FROM book_publication_value evidence
                     WHERE evidence.book_row_id=identifier.book_row_id AND evidence.kind='enrichment_isbn')
    UNION
    SELECT book_row_id,value,'inspection-reference','isbn' FROM book_publication_value WHERE kind='enrichment_isbn'
) SELECT book.content_hash,COALESCE(book.title,''),book.publication_metadata
         FROM book
         WHERE book.deleted_at IS NULL AND book.hidden_at IS NULL
               AND NOT EXISTS(SELECT 1 FROM pending_audible_enrichment audible WHERE audible.content_hash=book.content_hash) AND book.content_hash>COALESCE(?3,'')
           AND NOT EXISTS(SELECT 1 FROM lookup_isbn identifier WHERE identifier.book_row_id=book.row_id AND identifier.scheme='isbn' AND identifier.canonical_value IS NOT NULL)
           AND NOT EXISTS(
               SELECT 1 FROM external_metadata_attempt attempt
               WHERE attempt.book_row_id=book.row_id AND attempt.provider_id=?1
                 AND attempt.attempted_at >= CAST(strftime('%s','now') AS INTEGER)*1000-2592000000
                 AND (?2=0 OR attempt.status <> 'failed'))
           AND NOT EXISTS(SELECT 1 FROM book_subject s WHERE s.book_row_id=book.row_id AND lower(s.authority)='lcc')
         ORDER BY book.content_hash LIMIT ?4
