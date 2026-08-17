BEGIN;
SELECT pg_advisory_xact_lock(hashtext('bokheim-schema-upgrade'));

CREATE TABLE IF NOT EXISTS bokheim_schema_migration (
    version text PRIMARY KEY,
    applied_at timestamp with time zone NOT NULL DEFAULT now()
);

ALTER TABLE users ADD COLUMN IF NOT EXISTS is_admin boolean;
UPDATE users SET is_admin=false WHERE is_admin IS NULL;
ALTER TABLE users ALTER COLUMN is_admin SET DEFAULT false;
ALTER TABLE users ALTER COLUMN is_admin SET NOT NULL;

-- The encryption key did not exist in the earlier release, so legacy queued
-- plaintext tokens cannot be converted safely. Refuse the upgrade when such
-- mail is pending; an operator can let it drain or explicitly discard it.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema='public' AND table_name='account_email_outbox' AND column_name='token'
    ) AND NOT EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema='public' AND table_name='account_email_outbox' AND column_name='token_ciphertext'
    ) THEN
        IF EXISTS (SELECT 1 FROM account_email_outbox) THEN
            RAISE EXCEPTION 'pending legacy account email must drain before token encryption upgrade';
        END IF;
        ALTER TABLE account_email_outbox RENAME COLUMN token TO token_ciphertext;
    END IF;
END
$$;

ALTER TABLE pending_registration DROP CONSTRAINT IF EXISTS pending_registration_verification_hash_key;

CREATE TABLE IF NOT EXISTS user_thumbnail_charge (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    library_id text NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    content_hash text NOT NULL,
    size_bytes bigint NOT NULL CHECK (size_bytes >= 0),
    charged_at timestamp with time zone NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, content_hash)
);
CREATE INDEX IF NOT EXISTS idx_user_thumbnail_charge_library ON user_thumbnail_charge USING btree (library_id);

CREATE TABLE IF NOT EXISTS thumbnail_upload_reservation (
    id text PRIMARY KEY,
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    library_id text NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    content_hash text NOT NULL,
    size_bytes bigint NOT NULL CHECK (size_bytes >= 0),
    expires_at timestamp with time zone NOT NULL,
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    UNIQUE (user_id, content_hash)
);
CREATE INDEX IF NOT EXISTS idx_thumbnail_upload_reservation_expiry ON thumbnail_upload_reservation USING btree (expires_at);

CREATE TABLE IF NOT EXISTS admin_traffic_minute (
    bucket timestamp with time zone NOT NULL,
    route_group text NOT NULL,
    method text NOT NULL,
    status_class smallint NOT NULL CHECK (status_class BETWEEN 1 AND 5),
    request_count bigint NOT NULL CHECK (request_count >= 0),
    request_bytes bigint NOT NULL CHECK (request_bytes >= 0),
    response_bytes bigint NOT NULL CHECK (response_bytes >= 0),
    total_duration_ms bigint NOT NULL CHECK (total_duration_ms >= 0),
    max_duration_ms bigint NOT NULL CHECK (max_duration_ms >= 0),
    PRIMARY KEY (bucket, route_group, method, status_class)
);
CREATE INDEX IF NOT EXISTS idx_admin_traffic_minute_bucket ON admin_traffic_minute USING btree (bucket DESC);

CREATE TABLE IF NOT EXISTS admin_request_ip (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    occurred_at timestamp with time zone NOT NULL,
    client_ip inet NOT NULL,
    route_group text NOT NULL,
    method text NOT NULL,
    status_class smallint NOT NULL CHECK (status_class BETWEEN 1 AND 5),
    request_bytes bigint NOT NULL CHECK (request_bytes >= 0),
    response_bytes bigint NOT NULL CHECK (response_bytes >= 0),
    duration_ms bigint NOT NULL CHECK (duration_ms >= 0)
);
CREATE INDEX IF NOT EXISTS idx_admin_request_ip_time ON admin_request_ip USING btree (occurred_at DESC);
CREATE INDEX IF NOT EXISTS idx_admin_request_ip_address_time ON admin_request_ip USING btree (client_ip, occurred_at DESC);

CREATE TABLE IF NOT EXISTS admin_account_activity (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    first_seen_at timestamp with time zone NOT NULL,
    last_seen_at timestamp with time zone NOT NULL,
    request_count bigint NOT NULL CHECK (request_count > 0),
    CHECK (last_seen_at >= first_seen_at)
);
CREATE INDEX IF NOT EXISTS idx_admin_account_activity_last_seen ON admin_account_activity USING btree (last_seen_at DESC);

CREATE TABLE IF NOT EXISTS admin_account_activity_day (
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    activity_date date NOT NULL,
    request_count bigint NOT NULL CHECK (request_count >= 0),
    sync_request_count bigint NOT NULL CHECK (sync_request_count >= 0),
    CHECK (request_count + sync_request_count > 0),
    PRIMARY KEY (user_id, activity_date)
);
CREATE INDEX IF NOT EXISTS idx_admin_account_activity_day_date ON admin_account_activity_day USING btree (activity_date DESC);

CREATE TABLE IF NOT EXISTS admin_audit_event (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    administrator_user_id text NOT NULL REFERENCES users(id) ON DELETE RESTRICT,
    event_type text NOT NULL,
    window_started_at timestamp with time zone NOT NULL,
    occurred_at timestamp with time zone NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_admin_audit_event_time ON admin_audit_event USING btree (occurred_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_admin_audit_event_window ON admin_audit_event USING btree (administrator_user_id, event_type, window_started_at);

CREATE OR REPLACE FUNCTION reject_admin_audit_mutation() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'administrator audit events are append-only';
END;
$$;
DROP TRIGGER IF EXISTS admin_audit_event_append_only ON admin_audit_event;
CREATE TRIGGER admin_audit_event_append_only
BEFORE UPDATE OR DELETE ON admin_audit_event
FOR EACH ROW EXECUTE FUNCTION reject_admin_audit_mutation();

INSERT INTO bokheim_schema_migration(version) VALUES('2026-08-17-admin-and-token-encryption')
ON CONFLICT (version) DO NOTHING;
COMMIT;
