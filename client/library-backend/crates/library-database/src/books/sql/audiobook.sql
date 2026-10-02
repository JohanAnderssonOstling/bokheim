-- Cached audiobook playback metadata: chapters, duration, and recording narrator.

-- name: audiobook_playback_title_author?
-- param: content_hash: &str
SELECT COALESCE(title,'Untitled'), author FROM book WHERE content_hash = :content_hash;

-- name: audiobook_cached_metadata?
-- param: content_hash: &str
SELECT duration_ms, toc_json FROM book_navigation
WHERE content_hash = :content_hash AND duration_ms > 0;

-- name: audiobook_record_recording!
-- param: content_hash: &str
-- param: request_json: &str
INSERT INTO book_local_state(content_hash, recording_json) SELECT content_hash, :request_json FROM book WHERE content_hash = :content_hash
ON CONFLICT(content_hash) DO UPDATE SET recording_json = excluded.recording_json WHERE book_local_state.recording_json IS NOT excluded.recording_json;

-- name: audiobook_chapters_select?
-- param: content_hash: &str
SELECT toc_json, duration_ms FROM book_navigation
WHERE content_hash=:content_hash AND duration_ms IS NOT NULL;

-- name: audiobook_track_index_select?
-- param: content_hash: &str
SELECT tracks_json FROM book_navigation WHERE content_hash=:content_hash AND tracks_json IS NOT NULL;
