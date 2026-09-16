SELECT book_contributor.position, book_contributor.name, author_identity.stable_id, author_identity.preferred_name,
                author_external_identifier.authority, author_external_identifier.external_id
         FROM book_contributor
         LEFT JOIN author_identity ON author_identity.id=book_contributor.author_identity_id
         LEFT JOIN author_external_identifier ON author_external_identifier.author_identity_id=author_identity.id
         WHERE book_contributor.book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND book_contributor.role=X'617574'
         ORDER BY book_contributor.position, author_external_identifier.authority, author_external_identifier.external_id