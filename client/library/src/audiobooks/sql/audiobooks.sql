-- Audiobook metadata derived from local book files.

-- name: upsert_audiobook_metadata!
-- param: content_hash: &str
-- param: duration_ms: i64
INSERT INTO audiobook_metadata(content_hash,duration_ms)
VALUES(:content_hash,:duration_ms)
ON CONFLICT(content_hash) DO UPDATE SET
    duration_ms=excluded.duration_ms;
