-- Auth: username/password on users, bearer tokens, and signup invites.

ALTER TABLE users ADD COLUMN IF NOT EXISTS username      TEXT;
ALTER TABLE users ADD COLUMN IF NOT EXISTS password_hash TEXT;
ALTER TABLE users ADD COLUMN IF NOT EXISTS is_admin      BOOLEAN NOT NULL DEFAULT FALSE;

-- Usernames are optional (pre-auth users exist) but must be unique when set.
CREATE UNIQUE INDEX IF NOT EXISTS idx_users_username
    ON users(username)
    WHERE username IS NOT NULL;

CREATE TABLE IF NOT EXISTS auth_token (
    token_hash   TEXT PRIMARY KEY,            -- sha256 of the bearer token (hex)
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_auth_token_user ON auth_token(user_id);

CREATE TABLE IF NOT EXISTS invite (
    token_hash          TEXT PRIMARY KEY,
    note                TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at          TIMESTAMPTZ NOT NULL,
    consumed_by_user_id TEXT REFERENCES users(id)
);
