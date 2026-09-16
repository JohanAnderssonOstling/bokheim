UPDATE book SET title=?2,publication_metadata=?3
        WHERE content_hash=?1 AND (title IS NOT ?2 OR publication_metadata IS NOT ?3)