SELECT j.content_hash FROM json_each(?1) members
JOIN audible_enrichment_jobs j ON j.content_hash=members.value
JOIN book b ON b.content_hash=j.content_hash
WHERE j.policy=?3 AND b.deleted_at IS NULL AND b.hidden_at IS NULL
AND ((j.status IN ('pending','unavailable') AND j.retry_at<=?2)
 OR (j.status='resolving' AND j.lease_until<=?2))
ORDER BY j.content_hash
