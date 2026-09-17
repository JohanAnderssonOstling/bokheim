UPDATE sync_outbox
             SET body=(SELECT bokheim_mutation('book_lifecycle', book.content_hash, 0,
                         COALESCE(book.title, book.content_hash), contributors_for_book(book.row_id),
                         book.book_metadata,
                         COALESCE(book.added_at, 0), book.format, NULL)
                       FROM book WHERE book.content_hash=?1)
             WHERE state_kind='book_lifecycle' AND state_key=?1