SELECT book.content_hash FROM book_dir
                 JOIN book ON book.row_id = book_dir.book_row_id
                 WHERE book_dir.dir_id=?1 AND book.content_hash=?2
                   AND book_dir.deleted_at IS NULL