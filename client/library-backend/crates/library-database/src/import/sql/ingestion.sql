-- Ensure a file identity exists and retain its original addition time.

-- name: ingestion_ensure_book !
-- param: content_hash: &SqlParam<'_>
INSERT INTO book(content_hash)
VALUES(:content_hash)
ON CONFLICT(content_hash) DO NOTHING;

-- name: ingestion_added_at ->
-- param: content_hash: &SqlParam<'_>
-- param: fallback_time: &SqlParam<'_>
SELECT COALESCE(added_at,:fallback_time) FROM book WHERE content_hash=:content_hash;
