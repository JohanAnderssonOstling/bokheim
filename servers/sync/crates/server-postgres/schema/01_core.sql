-- Core identity tables shared by every other domain: every other schema
-- file's foreign keys ultimately point back to users/libraries, so this
-- file must be applied first.
--
-- This schema is applied to a freshly created, empty database on every
-- deploy (see servers/sync/deploy/install-server.sh) -- there is no migration
-- history to preserve. Edit these files directly; do not add incremental
-- migration files.

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
    name text NOT NULL DEFAULT 'Library'::text,
    -- A deleted UUID is retained permanently so stale replicas cannot
    -- recreate the same logical library through upsert or implicit sync.
    deleted_at timestamp with time zone
);
