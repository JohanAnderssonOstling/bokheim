SELECT position,name FROM book_contributor
             WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND role=X'617574'
             ORDER BY position