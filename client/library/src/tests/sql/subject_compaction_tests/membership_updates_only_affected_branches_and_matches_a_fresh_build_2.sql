UPDATE book_unified_concept SET concept_id=910002 WHERE book_row_id=(SELECT row_id FROM book WHERE title='Delta book');
