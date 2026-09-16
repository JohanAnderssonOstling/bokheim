SELECT DISTINCT book_row_id FROM book_dir WHERE deleted_at IS NULL AND is_downloaded=1 ORDER BY book_row_id
