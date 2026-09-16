INSERT INTO local_upload_preparation(content_hash,upload_id)
SELECT b.content_hash, json_extract(member.value,'$[1]')
FROM json_each(?1) member
JOIN book b ON b.content_hash=json_extract(member.value,'$[0]')
WHERE b.deleted_at IS NULL
AND NOT EXISTS(SELECT 1 FROM audible_enrichment_jobs j
    WHERE j.content_hash=b.content_hash AND j.policy=?2 AND j.status IN ('pending','resolving'))
ON CONFLICT(content_hash) DO UPDATE SET upload_id=excluded.upload_id
WHERE local_upload_preparation.upload_id IS NOT excluded.upload_id
