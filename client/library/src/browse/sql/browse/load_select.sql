SELECT book.content_hash,book.format,MAX(placement.is_downloaded)
             FROM book JOIN book_dir placement ON placement.book_row_id=book.row_id AND placement.deleted_at IS NULL
             WHERE book.deleted_at IS NULL AND book.hidden_at IS NULL
             GROUP BY book.row_id,book.content_hash,book.format