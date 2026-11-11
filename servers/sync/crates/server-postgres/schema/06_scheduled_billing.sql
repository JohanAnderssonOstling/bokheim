-- Keep the paid plan until the existing billing period ends. A later paid
-- period still needs an explicit access grant; a schedule never grants payment.
ALTER TABLE billing_subscription
    ADD COLUMN cancel_effective_at bigint CHECK (cancel_effective_at > 0),
    ADD COLUMN pending_plan_id text REFERENCES billing_plan(id),
    ADD COLUMN pending_change_at bigint CHECK (pending_change_at > 0),
    ADD CONSTRAINT billing_cancel_cutoff_requires_cancellation CHECK (cancel_effective_at IS NULL OR cancel_at_period_end),
    ADD CONSTRAINT billing_pending_plan_complete CHECK (
        (pending_plan_id IS NULL AND pending_change_at IS NULL) OR
        (pending_plan_id IS NOT NULL AND pending_change_at IS NOT NULL
         AND current_period_end IS NOT NULL AND pending_change_at<=current_period_end
         AND pending_plan_id<>plan_id AND NOT cancel_at_period_end)
    );

-- Backfill any known cancellation cutoff from the initial billing schema.
UPDATE billing_subscription SET cancel_effective_at=COALESCE(current_period_end, access_until)
WHERE cancel_at_period_end;

-- One clock-based projection drives both upload enforcement and the account
-- response. No worker or webhook is required to apply an already-known date.
CREATE VIEW billing_subscription_entitlement AS
WITH projected AS (
    SELECT subscription.user_id,
        CASE WHEN pending_change_at<=EXTRACT(EPOCH FROM statement_timestamp())
            THEN pending_plan_id ELSE plan_id END AS plan_id,
        CASE WHEN pending_change_at<=EXTRACT(EPOCH FROM statement_timestamp())
            THEN pending_plan.storage_quota_bytes ELSE plan.storage_quota_bytes END AS storage_quota_bytes,
        CASE WHEN cancel_at_period_end AND COALESCE(cancel_effective_at, current_period_end)<=EXTRACT(EPOCH FROM statement_timestamp())
            THEN 'canceled' ELSE status END AS status,
        current_period_end,
        COALESCE(cancel_at_period_end AND COALESCE(cancel_effective_at, current_period_end)>EXTRACT(EPOCH FROM statement_timestamp()), false) AS cancel_at_period_end,
        cancel_effective_at,
        -- LEAST ignores NULL in PostgreSQL: do not accidentally grant access to
        -- an unpaid subscription just because it has a cancellation date.
        CASE WHEN cancel_at_period_end AND access_until IS NOT NULL
            THEN LEAST(access_until, COALESCE(cancel_effective_at, current_period_end)) ELSE access_until END AS access_until,
        CASE WHEN pending_change_at>EXTRACT(EPOCH FROM statement_timestamp())
            THEN pending_plan_id END AS pending_plan_id,
        CASE WHEN pending_change_at>EXTRACT(EPOCH FROM statement_timestamp())
            THEN pending_plan.storage_quota_bytes END AS pending_plan_quota,
        CASE WHEN pending_change_at>EXTRACT(EPOCH FROM statement_timestamp())
            THEN pending_change_at END AS pending_change_at
    FROM billing_subscription subscription
    JOIN billing_plan plan ON plan.id=subscription.plan_id
    LEFT JOIN billing_plan pending_plan ON pending_plan.id=subscription.pending_plan_id
)
SELECT projected.*, billing_access_active(status, access_until) AS access_active
FROM projected;

CREATE OR REPLACE FUNCTION billing_storage_quota(account_user_id text, base_quota bigint) RETURNS bigint
LANGUAGE sql STABLE AS $$
    SELECT GREATEST(base_quota, COALESCE((
        SELECT storage_quota_bytes FROM billing_subscription_entitlement
        WHERE user_id=account_user_id AND access_active
    ), 0))
$$;
