-- name: activity_set_last_successful_sync_at!
-- param: value: &str
INSERT INTO sync_metadata(singleton, last_successful_sync_at) VALUES (1, :value)
ON CONFLICT(singleton) DO UPDATE SET last_successful_sync_at = excluded.last_successful_sync_at;

-- name: activity_begin_scan!
UPDATE library_local_state SET scan_state='pending' WHERE singleton=1;

-- name: activity_finish_scan!
-- param: state: &str
UPDATE library_local_state SET scan_state=:state WHERE singleton=1;

