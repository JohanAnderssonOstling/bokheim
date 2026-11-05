-- Cached audiobook playback metadata: chapters, duration, and recording narrator.

-- name: audiobook_cached_metadata?
-- param: content_hash: &BookId
SELECT b.duration_ms, CAST(b.toc_json AS BLOB), b.format,
       COALESCE(b.title,'Untitled'), b.author, local.recording_json,
       CAST(b.tracks_json AS BLOB)
FROM book b LEFT JOIN book_local_state local ON local.content_hash = b.content_hash
WHERE b.content_hash = :content_hash AND b.duration_ms > 0;

-- name: audiobook_record_recording!
-- param: content_hash: &BookId
-- param: request_json: &str
INSERT INTO book_local_state(content_hash, recording_json) SELECT content_hash, :request_json FROM book WHERE content_hash = :content_hash
ON CONFLICT(content_hash) DO UPDATE SET recording_json = excluded.recording_json WHERE book_local_state.recording_json IS NOT excluded.recording_json;

-- name: audiobook_track_index_select?
-- param: content_hash: &BookId
SELECT CAST(tracks_json AS BLOB) FROM book WHERE content_hash=:content_hash AND tracks_json IS NOT NULL;
