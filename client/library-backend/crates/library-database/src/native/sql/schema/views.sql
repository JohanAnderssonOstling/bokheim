-- name: schema_native_views &
CREATE VIEW IF NOT EXISTS pending_audible_enrichment AS
        SELECT content_hash FROM audible_enrichment_jobs WHERE status IN ('pending','resolving');
