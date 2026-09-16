SELECT
                 (SELECT deleted_at IS NOT NULL FROM dir WHERE id=?1),
                 (SELECT deleted_at IS NOT NULL FROM dir WHERE id=?2)