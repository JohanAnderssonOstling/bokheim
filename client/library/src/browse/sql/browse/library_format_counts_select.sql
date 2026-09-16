SELECT
             CASE lower(book.format)
                 WHEN 'pdf' THEN 2
                 WHEN 'm4b' THEN 3
                 ELSE 1
             END AS format_category,
             COUNT(*)
         FROM book
         WHERE book.deleted_at IS NULL
           AND book.hidden_at IS NULL
           AND EXISTS (
               SELECT 1
               FROM book_dir AS placement
               WHERE placement.book_row_id=book.row_id
                 AND placement.deleted_at IS NULL
           )
         GROUP BY format_category
         ORDER BY format_category