-- Permanent publication identities are account scoped. Blob checksums remain
-- global, immutable addresses of verified bytes.
--
-- The v14 baseline in 04_assets.sql already defines the table, so a fresh
-- install is a no-op; this only adds it to a database created before book
-- revisions existed. Books uploaded before this migration need no backfill:
-- an identity without a row resolves to itself.
CREATE TABLE IF NOT EXISTS user_book_revision (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content_hash text NOT NULL,
    blob_hash text NOT NULL REFERENCES blob_object(content_hash) ON DELETE RESTRICT,
    PRIMARY KEY (user_id, content_hash)
);
CREATE INDEX IF NOT EXISTS idx_user_book_revision_blob ON user_book_revision(user_id, blob_hash);
