-- name: schema_assets_views &
CREATE VIEW IF NOT EXISTS uploadable_covers AS
SELECT book.content_hash
FROM book JOIN book_transfer transfer ON transfer.content_hash=book.content_hash
WHERE book.deleted_at IS NULL
  AND NOT EXISTS(SELECT 1 FROM local_book_draft draft WHERE draft.content_hash=book.content_hash)
  AND transfer.cover_state='ready'
  AND transfer.server_has_book AND NOT transfer.server_has_cover
  AND NOT EXISTS(SELECT 1 FROM sync_outbox lifecycle
                 WHERE lifecycle.state_kind='book_lifecycle'
                   AND lifecycle.state_key=book.content_hash);
