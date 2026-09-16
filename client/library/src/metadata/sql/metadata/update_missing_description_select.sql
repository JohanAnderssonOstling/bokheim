SELECT EXISTS(SELECT 1 FROM book WHERE content_hash=?1 AND deleted_at IS NULL
          AND trim(COALESCE((SELECT description FROM book_description WHERE book_row_id=book.row_id),''))='')