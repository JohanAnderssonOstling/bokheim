-- Bearer tokens get a rolling expiry. NULL means "never" (legacy rows); new
-- rows always set it. resolve_token refuses expired tokens and extends the
-- window on each use.
ALTER TABLE auth_token ADD COLUMN IF NOT EXISTS expires_at TIMESTAMPTZ;
