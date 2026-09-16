SELECT bokheim_mutation('catalog_publication',catalog.source_id,catalog.edition_id,NULLIF(catalog.work_id,''),book.content_hash),0
                 FROM catalog_publication_book catalog JOIN book ON book.row_id=catalog.book_row_id
                 WHERE catalog.source_id=?1 AND catalog_publication_state_subkey(catalog.edition_id,NULLIF(catalog.work_id,''))=?2