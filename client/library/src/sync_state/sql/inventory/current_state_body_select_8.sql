SELECT bokheim_mutation('placement',book_dir.dir_id,book.content_hash,book_dir.deleted_at IS NULL,book_dir.trash_origin_dir_id),book_dir.deleted_at IS NOT NULL
                 FROM book_dir JOIN book ON book.row_id=book_dir.book_row_id
                 WHERE book.content_hash=?1 AND book_dir.dir_id=?2