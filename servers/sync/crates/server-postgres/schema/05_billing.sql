-- Provider-neutral billing. Prices, checkout sessions, payment credentials and
-- raw webhook payloads belong to a future provider adapter, not this schema.
-- Plan quotas are immutable; publish a new ID when changing an allowance.
CREATE TABLE billing_plan (
    id text PRIMARY KEY CHECK (length(id) BETWEEN 1 AND 64),
    storage_quota_bytes bigint NOT NULL CHECK (storage_quota_bytes > 0)
);

-- One current subscription per account. Revision is an optimistic concurrency
-- token for adapters fetching authoritative state, not a webhook timestamp.
CREATE TABLE billing_subscription (
    user_id text PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    provider text NOT NULL CHECK (length(provider) BETWEEN 1 AND 64),
    customer_id text NOT NULL CHECK (length(customer_id) BETWEEN 1 AND 255),
    subscription_id text NOT NULL CHECK (length(subscription_id) BETWEEN 1 AND 255),
    plan_id text NOT NULL REFERENCES billing_plan(id),
    status text NOT NULL CHECK (status IN ('pending', 'trialing', 'active', 'past_due', 'paused', 'canceled', 'expired')),
    current_period_end bigint CHECK (current_period_end > 0),
    cancel_at_period_end boolean NOT NULL,
    access_until bigint CHECK (access_until > 0),
    revision bigint NOT NULL CHECK (revision > 0),
    updated_at timestamp with time zone NOT NULL DEFAULT now(),
    UNIQUE (provider, customer_id),
    UNIQUE (provider, subscription_id)
);

-- A successful application and its receipt commit together. An adapter must
-- verify provider authenticity before submitting normalized events here.
CREATE TABLE billing_event (
    provider text NOT NULL,
    event_id text NOT NULL CHECK (length(event_id) BETWEEN 1 AND 255),
    user_id text NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    fingerprint bytea NOT NULL CHECK (octet_length(fingerprint)=32),
    snapshot jsonb NOT NULL,
    applied_revision bigint NOT NULL CHECK (applied_revision > 0),
    applied_at timestamp with time zone NOT NULL DEFAULT now(),
    PRIMARY KEY (provider, event_id)
);

CREATE INDEX billing_event_account ON billing_event(user_id);

-- Survives identity deletion, so deleting an account cannot forget the external
-- subscription. Jobs also capture subscriptions superseded by a replacement.
CREATE TABLE billing_cancellation_outbox (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    provider text NOT NULL,
    customer_id text NOT NULL,
    subscription_id text NOT NULL,
    created_at timestamp with time zone NOT NULL DEFAULT now(),
    available_at timestamp with time zone NOT NULL DEFAULT now(),
    lease_until timestamp with time zone,
    lease_token uuid,
    attempts integer NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    completed_at timestamp with time zone,
    UNIQUE (provider, subscription_id)
);

CREATE INDEX billing_pending_cancellations ON billing_cancellation_outbox(provider, available_at, id)
WHERE completed_at IS NULL;

CREATE FUNCTION billing_queue_account_cancellation() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    INSERT INTO billing_cancellation_outbox(provider, customer_id, subscription_id)
    SELECT provider, customer_id, subscription_id FROM billing_subscription WHERE user_id=OLD.id
    ON CONFLICT (provider, subscription_id) DO NOTHING;
    RETURN OLD;
END;
$$;

CREATE TRIGGER billing_cancel_before_account_deletion BEFORE DELETE ON users
FOR EACH ROW EXECUTE FUNCTION billing_queue_account_cancellation();

-- Grant only explicitly time-bounded access. Canceled subscriptions can retain
-- prepaid access; past-due grace, if any, is an explicit adapter decision.
CREATE FUNCTION billing_access_active(subscription_status text, access_until bigint) RETURNS boolean
LANGUAGE sql STABLE AS $$
    SELECT COALESCE(subscription_status IN ('trialing', 'active', 'past_due', 'canceled')
        AND access_until > EXTRACT(EPOCH FROM statement_timestamp()), false)
$$;

-- Compute entitlement at admission time, including when no new webhook arrives.
-- The existing quota column remains the base allowance and is never overwritten.
CREATE FUNCTION billing_storage_quota(account_user_id text, base_quota bigint) RETURNS bigint
LANGUAGE sql STABLE AS $$
    SELECT GREATEST(base_quota, COALESCE((
        SELECT plan.storage_quota_bytes
        FROM billing_subscription subscription JOIN billing_plan plan ON plan.id=subscription.plan_id
        WHERE subscription.user_id=account_user_id
          AND billing_access_active(subscription.status, subscription.access_until)
    ), 0))
$$;
