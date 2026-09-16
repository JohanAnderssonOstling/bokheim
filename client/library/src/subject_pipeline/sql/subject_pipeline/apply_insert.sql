INSERT INTO local_subject_pipeline(content_hash,state,metadata,title,complete,retry_at)
        SELECT content_hash,?2,publication_metadata,COALESCE(title,''),?3,CASE WHEN ?3=1 AND ?4=0 THEN 0 ELSE CAST(strftime('%s','now') AS INTEGER)+?4 END FROM book
        WHERE content_hash=?1 AND deleted_at IS NULL AND (publication_metadata=?5 OR (?7 AND length(publication_metadata)=0)) AND COALESCE(title,'')=?6
        ON CONFLICT(content_hash) DO UPDATE SET state=excluded.state,metadata=excluded.metadata,title=excluded.title,complete=excluded.complete,retry_at=excluded.retry_at