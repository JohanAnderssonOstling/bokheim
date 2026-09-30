-- name: load_book_cards?
-- param: book_ids: &str
-- What a card shows, for books a browse query has already chosen. `book_ids`
-- is a JSON array of book row ids; cards come back in its order, since
-- json_each keys are array positions.
SELECT book.content_hash,book.title,book.subtitle,book.author,
       book.read_progress AS progress,book.format,book.added_at,
       EXISTS(SELECT 1 FROM book_dir placement
              WHERE placement.book_row_id=book.row_id AND placement.deleted_at IS NULL
                AND placement.is_downloaded=1) AS downloaded,
       COALESCE(transfer.book_download IS NOT NULL, 0) AS download_requested,
       audio.duration_ms AS audiobook_duration_ms
FROM json_each(:book_ids) selected
JOIN book ON book.row_id=selected.value
LEFT JOIN book_transfer transfer ON transfer.content_hash=book.content_hash
LEFT JOIN audiobook_metadata audio ON audio.content_hash=book.content_hash
ORDER BY selected.key;
