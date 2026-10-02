-- Table-of-contents reads.

-- name: book_toc_select?
-- param: content_hash: &str
SELECT toc_json FROM book_navigation WHERE content_hash=:content_hash;

-- name: reading_entry_select?
-- param: content_hash: &str
SELECT reading_target FROM book_local_state WHERE content_hash=:content_hash AND reading_target IS NOT NULL;

-- Complete synchronized navigation documents replace all optional fields.
-- name: navigation_upsert!
-- param: content_hash: &str
-- param: entry_count: i64
-- param: toc_json: &str
-- param: duration_ms: Option<i64>
-- param: tracks_json: Option<&str>
INSERT INTO book_navigation(content_hash,entry_count,toc_json,duration_ms,tracks_json)
VALUES(:content_hash,:entry_count,:toc_json,:duration_ms,:tracks_json)
ON CONFLICT(content_hash) DO UPDATE SET
    entry_count=excluded.entry_count, toc_json=excluded.toc_json,
    duration_ms=excluded.duration_ms, tracks_json=excluded.tracks_json;

-- Enrichment replaces navigation entries while preserving playback facts.
-- name: navigation_update_entries!
-- param: content_hash: &str
-- param: entry_count: i64
-- param: toc_json: &str
UPDATE book_navigation SET entry_count=:entry_count,toc_json=:toc_json
WHERE content_hash=:content_hash AND duration_ms IS NOT NULL;
