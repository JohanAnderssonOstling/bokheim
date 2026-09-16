SELECT DISTINCT publication.source_id,publication.edition_id,publication.work_id
         FROM catalog_publication_book publication
         JOIN book ON book.row_id=publication.book_row_id
         WHERE book.deleted_at IS NULL
         ORDER BY publication.source_id,publication.edition_id,publication.work_id