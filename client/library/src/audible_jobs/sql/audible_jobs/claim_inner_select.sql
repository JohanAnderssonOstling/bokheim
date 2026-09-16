SELECT content_hash,request_json
        FROM audible_enrichment_jobs
        WHERE ((status IN ('pending','unavailable') AND retry_at<=?1)
           OR (status='resolving' AND lease_until<=?1))
          AND (?2=0 OR (policy=?3 AND EXISTS(SELECT 1 FROM book b WHERE b.content_hash=audible_enrichment_jobs.content_hash AND b.deleted_at IS NULL AND b.hidden_at IS NULL)))
          AND (?4 IS NULL OR content_hash IN (SELECT value FROM json_each(?4)))
        ORDER BY updated_at,content_hash LIMIT 1