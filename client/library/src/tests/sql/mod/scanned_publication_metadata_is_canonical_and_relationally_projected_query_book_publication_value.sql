SELECT COUNT(*) FROM book_publication_value WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1))
