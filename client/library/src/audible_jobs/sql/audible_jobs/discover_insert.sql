INSERT INTO audible_enrichment_jobs(content_hash,policy,request_json,status,updated_at)
        VALUES(?1,?2,?3,'pending',?4)
        ON CONFLICT(content_hash) DO UPDATE SET policy=excluded.policy,request_json=excluded.request_json,status='pending',response_json=NULL,
        claim_token=NULL,lease_until=NULL,retry_at=0,updated_at=excluded.updated_at
        WHERE audible_enrichment_jobs.policy<>excluded.policy OR audible_enrichment_jobs.request_json<>excluded.request_json