SELECT COUNT(*) FROM book_publication_date WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1))
