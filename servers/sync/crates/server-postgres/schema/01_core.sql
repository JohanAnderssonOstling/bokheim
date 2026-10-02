-- Core identity tables shared by every other domain: every other schema
-- file's foreign keys ultimately point back to users/libraries, so this
-- file must be applied first.
--
-- A new installation starts at schema version 4.
CREATE TABLE bokheim_schema_version (
    version integer PRIMARY KEY CHECK (version = 4)
);
INSERT INTO bokheim_schema_version(version) VALUES (4);

CREATE TABLE users (
    id text PRIMARY KEY,
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    password_hash text,
    email text
);

CREATE UNIQUE INDEX idx_users_email ON users USING btree (email) WHERE (email IS NOT NULL);

CREATE TABLE libraries (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    name text NOT NULL,
    -- A deleted UUID is retained permanently so stale replicas cannot
    -- recreate the same logical library through upsert or implicit sync.
    deleted_at timestamp with time zone,
    cloud_storage_enabled boolean NOT NULL DEFAULT true
);
