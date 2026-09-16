SELECT author_identity.id, author_identity.stable_id, author_identity.is_provisional
         FROM book_contributor JOIN author_identity ON author_identity.id=book_contributor.author_identity_id
         WHERE book_contributor.book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND book_contributor.position=?2 AND book_contributor.role=X'617574'