SELECT book_row_id,position,name,code FROM book_subject
             WHERE LOWER(TRIM(authority))=?1 ORDER BY book_row_id,position