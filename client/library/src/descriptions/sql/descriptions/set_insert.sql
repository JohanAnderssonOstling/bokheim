INSERT INTO book_description(book_row_id,description)
         SELECT row_id,?2 FROM book WHERE content_hash=?1
           AND (?2<>'' OR EXISTS(SELECT 1 FROM book_description WHERE book_row_id=book.row_id))
         ON CONFLICT(book_row_id) DO UPDATE SET description=excluded.description
         WHERE book_description.description IS NOT excluded.description