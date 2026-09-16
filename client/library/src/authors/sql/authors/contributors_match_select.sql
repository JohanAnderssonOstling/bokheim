SELECT c.position,a.stable_id,c.name,c.sort_name,c.role
        FROM book_contributor c LEFT JOIN author_identity a ON a.id=c.author_identity_id
        WHERE c.book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) ORDER BY c.position