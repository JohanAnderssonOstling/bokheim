SELECT book.read_pos, reading.changed_at, book.title, book_contributor.name
             FROM book
             JOIN sync_state_version reading ON reading.state_kind='reading_position' AND reading.state_key=book.content_hash
             JOIN book_contributor ON book_contributor.book_row_id=book.row_id AND book_contributor.position=0
             WHERE book.content_hash=printf('%064x',1)