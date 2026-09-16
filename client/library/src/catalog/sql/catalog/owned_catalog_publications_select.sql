SELECT EXISTS(
             SELECT 1
             FROM catalog_publication_book AS publication
             JOIN book ON book.row_id=publication.book_row_id
             WHERE publication.source_id=?1
               AND publication.edition_id=?2
               AND publication.work_id=?3
               AND book.deleted_at IS NULL
         )