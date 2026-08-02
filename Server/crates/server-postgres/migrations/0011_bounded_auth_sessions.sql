-- Authentication sessions have an absolute lifetime. Older rows created before
-- expiry was introduced are bounded relative to their creation time as well.
UPDATE auth_token
SET expires_at = created_at + INTERVAL '90 days'
WHERE expires_at IS NULL;

ALTER TABLE auth_token
    ALTER COLUMN expires_at SET NOT NULL;

CREATE INDEX IF NOT EXISTS idx_auth_token_expiry
    ON auth_token(expires_at);

