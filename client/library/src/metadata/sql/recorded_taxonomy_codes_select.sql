SELECT DISTINCT subject.system_id,subject.code
         FROM book_subject_code subject
         JOIN book ON book.row_id=subject.book_row_id
         WHERE book.content_hash=?1
         ORDER BY subject.system_id,subject.code