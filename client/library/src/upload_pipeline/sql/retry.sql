SELECT EXISTS(
    SELECT 1 FROM external_metadata_attempt a JOIN book b ON b.row_id=a.book_row_id
    WHERE a.status='failed' AND b.deleted_at IS NULL AND b.hidden_at IS NULL
)
OR EXISTS(
    SELECT 1 FROM local_subject_pipeline p JOIN book b ON b.content_hash=p.content_hash
    WHERE p.retry_at>0 AND b.deleted_at IS NULL AND b.hidden_at IS NULL
)
OR EXISTS(
    SELECT 1 FROM audible_enrichment_jobs j JOIN book b ON b.content_hash=j.content_hash
    WHERE j.policy=?1 AND j.status IN ('pending','resolving','unavailable')
    AND b.deleted_at IS NULL AND b.hidden_at IS NULL
)
