UPDATE book_dir SET is_downloaded=1 WHERE book_row_id=(SELECT row_id FROM book WHERE title='Alpha book')
