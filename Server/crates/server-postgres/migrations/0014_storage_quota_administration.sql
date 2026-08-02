-- A plan downgrade may legitimately leave an account over quota. Admission
-- transactions reject new reservations until usage falls below the new limit;
-- existing content remains readable and is never deleted by a quota change.
ALTER TABLE user_storage_account
    DROP CONSTRAINT IF EXISTS user_storage_account_check;

CREATE TABLE storage_quota_change (
    change_id TEXT PRIMARY KEY CHECK (char_length(change_id) BETWEEN 1 AND 128),
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    actor TEXT NOT NULL CHECK (char_length(actor) BETWEEN 1 AND 128),
    previous_quota_bytes BIGINT NOT NULL CHECK (previous_quota_bytes >= 0),
    new_quota_bytes BIGINT NOT NULL CHECK (new_quota_bytes >= 0),
    reason TEXT NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 500),
    changed_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX storage_quota_change_user_time
    ON storage_quota_change(user_id, changed_at DESC);
