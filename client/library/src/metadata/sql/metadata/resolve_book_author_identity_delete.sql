DELETE FROM author_identity
             WHERE id=?1 AND is_provisional=1
               AND NOT EXISTS(SELECT 1 FROM book_contributor WHERE author_identity_id=?1)
               AND NOT EXISTS(SELECT 1 FROM author_external_identifier WHERE author_identity_id=?1)