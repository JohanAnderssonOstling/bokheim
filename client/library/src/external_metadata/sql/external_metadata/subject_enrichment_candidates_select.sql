SELECT book.content_hash, COALESCE(book.title, ''), MIN(identifier.canonical_value)
         FROM book
         JOIN book_identifier identifier ON identifier.book_row_id=book.row_id
         WHERE book.deleted_at IS NULL
           AND book.hidden_at IS NULL
           AND identifier.scheme='isbn'
           AND identifier.canonical_value IS NOT NULL
           AND NOT EXISTS(SELECT 1 FROM book_subject subject WHERE subject.book_row_id=book.row_id)
           AND NOT EXISTS(
               SELECT 1 FROM external_metadata_attempt attempt
               WHERE attempt.book_row_id=book.row_id
                 AND attempt.provider_id=?1
                 AND (?2=0 OR attempt.status <> 'failed')
           )
         GROUP BY book.row_id
         ORDER BY LOWER(COALESCE(book.title, '')), book.content_hash
         LIMIT ?3