DELETE FROM book_unified_concept WHERE book_row_id=(SELECT row_id FROM book WHERE title='Alpha book');
