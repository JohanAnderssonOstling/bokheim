SELECT author_identity.id, author_identity.stable_id
             FROM author_external_identifier JOIN author_identity ON author_identity.id=author_external_identifier.author_identity_id
             WHERE author_external_identifier.authority=?1 AND author_external_identifier.external_id=?2