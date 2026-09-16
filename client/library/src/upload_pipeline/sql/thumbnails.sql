SELECT w.content_hash FROM json_each(?1) members
JOIN local_thumbnail_work w ON w.content_hash=members.value
WHERE w.state='pending' AND w.retry_after<=?2
ORDER BY w.content_hash
