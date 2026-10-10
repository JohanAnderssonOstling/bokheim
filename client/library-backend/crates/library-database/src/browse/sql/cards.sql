-- name: load_book_cards?
-- param: book_ids: &str
-- What a card shows, for books a browse query has already chosen. `book_ids`
-- is a JSON array of book row ids; cards come back in its order, since
-- json_each keys are array positions.
SELECT book.content_hash,book.title,book.subtitle,book.author,
       book.read_progress AS progress,book.format,book.added_at,
       COALESCE((SELECT downloaded FROM library_book_presence WHERE book_row_id=book.row_id),0) AS downloaded,
       COALESCE(transfer.book_download IS NOT NULL, 0) AS download_requested,
       book.duration_ms AS audiobook_duration_ms
FROM json_each(:book_ids) selected
JOIN book ON book.row_id=selected.value
LEFT JOIN book_transfer transfer ON transfer.content_hash=book.content_hash
ORDER BY selected.key;
