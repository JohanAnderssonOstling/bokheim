-- Local thumbnail-recovery work and its durable result state.

-- name: request_thumbnail!
-- param: content_hash: &str
INSERT INTO local_thumbnail_work(content_hash, state) VALUES (:content_hash, 'pending')
ON CONFLICT(content_hash) DO UPDATE SET state = 'pending', retry_after = 0;

-- name: thumbnail_work_state ->
-- param: content_hash: &str
SELECT state, retry_after FROM local_thumbnail_work WHERE content_hash = :content_hash;

-- name: record_thumbnail_source!
-- param: content_hash: &str
-- param: origin: &str
INSERT INTO local_thumbnail_source(content_hash, origin) VALUES (:content_hash, :origin)
ON CONFLICT(content_hash) DO UPDATE SET origin = excluded.origin;

-- name: set_thumbnail_result!
-- param: content_hash: &str
-- param: state: &str
INSERT INTO local_thumbnail_work(content_hash, state) VALUES (:content_hash, :state)
ON CONFLICT(content_hash) DO UPDATE SET state = excluded.state, retry_after = 0;

-- name: retry_thumbnail!
-- param: content_hash: &str
-- param: retry_after: i64
INSERT INTO local_thumbnail_work(content_hash, state, retry_after) VALUES (:content_hash, 'pending', :retry_after)
ON CONFLICT(content_hash) DO UPDATE SET state = 'pending', retry_after = excluded.retry_after;

-- name: has_pending_thumbnail_work ->
SELECT EXISTS(SELECT 1 FROM local_thumbnail_work WHERE state = 'pending');

-- name: request_missing_thumbnail!
-- param: content_hash: &str
INSERT INTO local_thumbnail_work(content_hash, state) VALUES (:content_hash, 'pending')
ON CONFLICT(content_hash) DO UPDATE SET state = 'pending', retry_after = 0
WHERE local_thumbnail_work.state = 'ready';

-- name: pending_thumbnail_jobs_page?
-- param: after: &str
SELECT content_hash, retry_after FROM local_thumbnail_work
WHERE state = 'pending' AND content_hash > :after
ORDER BY content_hash LIMIT 32;

-- name: pending_thumbnail_job_count ->
SELECT COUNT(*) FROM local_thumbnail_work WHERE state = 'pending';

-- name: has_remote_thumbnail ->
-- param: content_hash: &str
SELECT EXISTS(SELECT 1 FROM remote_asset WHERE kind = 'thumbnail' AND hash = :content_hash);

-- name: discard_thumbnail_work!
-- param: content_hash: &str
DELETE FROM local_thumbnail_work WHERE content_hash = :content_hash;
