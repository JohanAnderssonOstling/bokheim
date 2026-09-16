SELECT LOWER(COALESCE(authority,'')),COALESCE(code,name)
                     FROM book_subject
                     WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)
                       AND LOWER(COALESCE(authority,''))='lcc'
                       AND source IN ('dc:subject','openlibrary:metadata:sibling-edition','openlibrary:metadata:work')