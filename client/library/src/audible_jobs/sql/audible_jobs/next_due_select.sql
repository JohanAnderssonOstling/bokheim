SELECT MIN(CASE WHEN j.status='resolving' THEN lease_until ELSE retry_at END)
        FROM audible_enrichment_jobs j JOIN book b ON b.content_hash=j.content_hash
        WHERE j.status IN ('pending','unavailable','resolving') AND b.deleted_at IS NULL AND b.hidden_at IS NULL AND j.policy=?1