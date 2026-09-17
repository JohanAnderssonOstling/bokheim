WITH lookup_isbn AS (
    SELECT book_row_id,canonical_value,source,'isbn' AS scheme FROM book_identifier identifier
    WHERE scheme='isbn' AND canonical_value IS NOT NULL
      AND NOT EXISTS(SELECT 1 FROM book_value evidence
                     WHERE evidence.book_row_id=identifier.book_row_id AND evidence.kind='enrichment_isbn')
    UNION
    SELECT book_row_id,value,'inspection-reference','isbn' FROM book_value WHERE kind='enrichment_isbn'
), candidates AS (
             SELECT book.row_id,book.content_hash,(
                 SELECT COUNT(*) FROM book_subject subject
                 WHERE subject.book_row_id=book.row_id
                   AND LOWER(COALESCE(subject.authority,''))='lcc'
             ) AS classification_count
             FROM book
             WHERE book.deleted_at IS NULL AND book.content_hash>COALESCE(?4,'')
               AND book.hidden_at IS NULL
               AND NOT EXISTS(SELECT 1 FROM pending_audible_enrichment audible WHERE audible.content_hash=book.content_hash)
               AND (EXISTS(
                   SELECT 1 FROM lookup_isbn identifier
                   WHERE identifier.book_row_id=book.row_id
                     AND identifier.scheme='isbn'
                     AND identifier.canonical_value IS NOT NULL
               ) OR EXISTS(SELECT 1 FROM local_import_inspection inspection WHERE inspection.content_hash=book.content_hash AND inspection.version>=2))
               AND NOT EXISTS(
                   SELECT 1 FROM external_metadata_attempt attempt
                   WHERE attempt.book_row_id=book.row_id
                     AND attempt.provider_id=?1
                     AND (attempt.identifier<>'' OR NOT EXISTS(
                         SELECT 1 FROM lookup_isbn WHERE book_row_id=book.row_id AND scheme='isbn' AND canonical_value IS NOT NULL
                     ))
                     AND attempt.attempted_at >= CAST(strftime('%s','now') AS INTEGER)*1000-2592000000
                     AND (?2=0 OR attempt.status <> 'failed')
               )
             AND NOT EXISTS(SELECT 1 FROM book_subject s WHERE s.book_row_id=book.row_id AND lower(s.authority)='lcc')
             ORDER BY classification_count DESC,book.content_hash
             LIMIT ?3
         )
         SELECT candidates.content_hash,identifier.canonical_value,identifier.source,candidates.classification_count
         FROM candidates
         LEFT JOIN lookup_isbn identifier ON identifier.book_row_id=candidates.row_id AND identifier.scheme='isbn' AND identifier.canonical_value IS NOT NULL
         ORDER BY candidates.classification_count DESC,candidates.content_hash,identifier.canonical_value
