-- Version 10: the account's OPDS feeds (see 01_core.sql).
CREATE TABLE opds_feeds (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name text NOT NULL,
    url text NOT NULL,
    deleted_at timestamp with time zone
);

CREATE INDEX idx_opds_feeds_user ON opds_feeds USING btree (user_id);

ALTER TABLE bokheim_schema_version DROP CONSTRAINT bokheim_schema_version_version_check;
UPDATE bokheim_schema_version SET version=10;
ALTER TABLE bokheim_schema_version ADD CONSTRAINT bokheim_schema_version_version_check CHECK(version=10);
