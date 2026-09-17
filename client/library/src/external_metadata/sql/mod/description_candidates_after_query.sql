WITH lookup_isbn AS (
    SELECT book_row_id,canonical_value,source,'isbn' AS scheme FROM book_identifier identifier
    WHERE scheme='isbn' AND canonical_value IS NOT NULL
      AND NOT EXISTS(SELECT 1 FROM book_value evidence
                     WHERE evidence.book_row_id=identifier.book_row_id AND evidence.kind='enrichment_isbn')
    UNION
    SELECT book_row_id,value,'inspection-reference','isbn' FROM book_value WHERE kind='enrichment_isbn'
), candidates AS (
        SELECT b.row_id,b.content_hash,group_concat(DISTINCT i.canonical_value ORDER BY i.canonical_value) AS isbns FROM book b
        JOIN lookup_isbn i ON i.book_row_id=b.row_id AND i.scheme='isbn' AND i.canonical_value IS NOT NULL
        WHERE b.deleted_at IS NULL AND b.hidden_at IS NULL AND trim(COALESCE((SELECT description FROM book_description WHERE book_row_id=b.row_id),''))='' AND b.content_hash>?1 AND (?4 IS NULL OR b.content_hash IN (SELECT value FROM json_each(?4)))
        AND NOT EXISTS(SELECT 1 FROM pending_audible_enrichment a WHERE a.content_hash=b.content_hash)
        GROUP BY b.row_id
    ) SELECT c.content_hash,c.isbns FROM candidates c LEFT JOIN external_metadata_attempt a ON a.book_row_id=c.row_id AND a.provider_id=?2
    WHERE a.book_row_id IS NULL OR a.identifier<>c.isbns OR a.status='updated'
       OR a.attempted_at<CAST(strftime('%s','now') AS INTEGER)*1000-2592000000
       OR (a.status='failed' AND a.attempted_at<CAST(strftime('%s','now') AS INTEGER)*1000-60000)
    ORDER BY c.content_hash LIMIT ?3