SELECT typeof(book.row_id), typeof(book_dir.book_row_id), book.content_hash
             FROM book_dir JOIN book ON book.row_id = book_dir.book_row_id