-- Audiobook metadata derived from local publication files.

-- name: upsert_audiobook_metadata!
-- param: content_hash: &str
-- param: duration_ms: i64
-- param: chapter_count: i64
-- param: chapter_source: &str
-- param: chapters_json: &str
INSERT INTO audiobook_metadata(content_hash,duration_ms,chapter_count,chapter_source,chapters_json)
VALUES(:content_hash,:duration_ms,:chapter_count,:chapter_source,:chapters_json)
ON CONFLICT(content_hash) DO UPDATE SET
    duration_ms=excluded.duration_ms,
    chapter_count=excluded.chapter_count,
    chapter_source=excluded.chapter_source,
    chapters_json=excluded.chapters_json;
