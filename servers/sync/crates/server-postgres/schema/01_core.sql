-- Core identity tables shared by every other domain: every other schema
-- file's foreign keys ultimately point back to users/libraries, so this
-- file must be applied first.
--
-- This is the complete protocol-v14 baseline schema. Databases created from
-- it carry an explicit baseline marker; `sync-server upgrade` accepts that
-- marker and applies only immutable post-v14 migrations.

CREATE TABLE bokheim_schema_migration (
    version text PRIMARY KEY,
    applied_at timestamp with time zone NOT NULL DEFAULT now()
);

INSERT INTO bokheim_schema_migration(version) VALUES ('14-baseline');

CREATE TABLE users (
    id text PRIMARY KEY,
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    password_hash text,
    email text,
    is_admin boolean NOT NULL DEFAULT false
);

CREATE UNIQUE INDEX idx_users_email ON users USING btree (email) WHERE (email IS NOT NULL);

CREATE TABLE libraries (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    name text NOT NULL DEFAULT 'Library'::text,
    -- A deleted UUID is retained permanently so stale replicas cannot
    -- recreate the same logical library through upsert or implicit sync.
    deleted_at timestamp with time zone
);
