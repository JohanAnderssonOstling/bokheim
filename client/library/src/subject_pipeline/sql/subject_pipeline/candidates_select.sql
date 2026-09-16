SELECT b.content_hash,COALESCE(b.title,''),b.format,b.publication_metadata,
        CASE WHEN s.metadata=b.publication_metadata AND s.title=COALESCE(b.title,'')
             AND NOT(s.retry_at>0 AND COALESCE(json_extract(s.state,'$.inspection_revision'),0)<>?3)
             AND NOT(s.complete=1 AND s.retry_at>0 AND s.retry_at<=CAST(strftime('%s','now') AS INTEGER)) THEN s.state ELSE NULL END
        FROM book b LEFT JOIN local_subject_pipeline s USING(content_hash)
        WHERE (?4 IS NULL OR b.content_hash IN (SELECT value FROM json_each(?4))) AND b.deleted_at IS NULL AND b.hidden_at IS NULL AND b.content_hash>?1
        AND NOT EXISTS(SELECT 1 FROM pending_audible_enrichment a WHERE a.content_hash=b.content_hash)
        AND (s.content_hash IS NULL OR s.metadata<>b.publication_metadata OR s.title<>COALESCE(b.title,'')
             OR (s.retry_at>0 AND COALESCE(json_extract(s.state,'$.inspection_revision'),0)<>?3)
             OR ((s.complete=0 OR s.retry_at>0) AND s.retry_at<=CAST(strftime('%s','now') AS INTEGER)))
        ORDER BY b.content_hash LIMIT ?2
