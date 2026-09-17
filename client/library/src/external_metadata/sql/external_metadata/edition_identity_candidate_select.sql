SELECT COALESCE(book.title,''),book.book_metadata
             FROM book
             WHERE book.content_hash=?1 AND book.deleted_at IS NULL AND book.hidden_at IS NULL
               AND NOT EXISTS(SELECT 1 FROM book_identifier identifier WHERE identifier.book_row_id=book.row_id AND identifier.scheme='isbn' AND identifier.canonical_value IS NOT NULL)