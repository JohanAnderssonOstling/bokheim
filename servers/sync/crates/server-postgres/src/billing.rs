//! Provider-neutral billing persistence. Only trusted server adapters may mutate
//! subscriptions; clients can read entitlements but cannot grant themselves access.

use crate::PostgresDatabase;
use account_contract::{AccountEntitlementsResponse, AccountSubscription, ScheduledStoragePlanChange, StoragePlan, SubscriptionStatus};
use serde::{Deserialize, Serialize};
use sqlx::{Error as SqlxError, Postgres, Row, Transaction};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum BillingError {
    #[error("{0}")]
    InvalidInput(&'static str),
    #[error("billing account does not exist")]
    UnknownAccount,
    #[error("billing plan does not exist")]
    UnknownPlan,
    #[error("plan identifiers cannot be reused with a different quota")]
    PlanConflict,
    #[error("subscription revision changed to {actual_revision}; fetch authoritative state again")]
    RevisionConflict { actual_revision: i64 },
    #[error("billing event identifier was reused with a different source event or account")]
    EventConflict,
    #[error("subscription has been retired for cancellation")]
    CancellationPending,
    #[error("external subscription belongs to a live account")]
    SubscriptionOwnedByLiveAccount,
    #[error("replacement must preserve the existing paid allowance through {until}")]
    PaidPeriodProtected { until: i64 },
    #[error(transparent)]
    Database(#[from] SqlxError),
    #[error(transparent)]
    Encoding(#[from] serde_json::Error),
}

/// A normalized, authoritative provider snapshot. The adapter decides which
/// verified payments/trials/grace periods justify the explicit access deadline.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubscriptionSnapshot {
    pub user_id: String,
    pub provider: String,
    pub customer_id: String,
    pub subscription_id: String,
    pub plan_id: String,
    pub pending_plan_change: Option<PendingPlanChange>,
    pub status: SubscriptionStatus,
    pub current_period_end: Option<i64>,
    pub cancel_at_period_end: bool,
    pub cancel_effective_at: Option<i64>,
    pub access_until: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingPlanChange {
    pub plan_id: String,
    pub effective_at: i64,
}

#[derive(Clone, Debug)]
pub enum SubscriptionChange {
    CancelAtPeriodEnd,
    Downgrade { plan_id: String },
    KeepCurrentPlan,
}

/// A trusted adapter submits this AFTER confirming the same change externally.
/// The request is not exposed directly to an authenticated account client.
pub struct SubscriptionChangeRequest {
    pub user_id: String,
    pub provider: String,
    pub subscription_id: String,
    pub event_id: String,
    pub fingerprint: [u8; 32],
    pub expected_revision: i64,
    pub change: SubscriptionChange,
}

#[derive(Clone, Debug)]
pub struct StoredSubscription {
    pub revision: i64,
    pub snapshot: SubscriptionSnapshot,
}

/// Read the local revision BEFORE fetching fresh provider state. On a revision
/// conflict, refetch both; never just retry the stale snapshot with a new token.
pub struct SubscriptionEvent {
    pub event_id: String,
    /// Stable 32-byte digest of the authenticated source event, excluding
    /// delivery headers. A retry may fetch newer state but has the same digest.
    pub fingerprint: [u8; 32],
    pub expected_revision: i64,
    pub snapshot: SubscriptionSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplySubscriptionResult {
    Applied {
        revision: i64,
    },
    Duplicate {
        revision: i64,
    },
    /// The account was deleted before this verified subscription arrived.
    /// Cleanup is durably recorded; this result grants no account access.
    OrphanCancellationRecorded,
}

#[derive(Debug)]
pub struct CancellationJob {
    pub id: i64,
    pub lease_token: Uuid,
    pub provider: String,
    pub customer_id: String,
    pub subscription_id: String,
}

#[derive(Clone)]
pub struct PostgresBillingService {
    database: PostgresDatabase,
}

impl PostgresBillingService {
    pub fn new(database: PostgresDatabase) -> Self {
        Self { database }
    }

    /// Idempotent registration of an immutable plan. Prices live in adapters.
    pub async fn register_plan(&self, plan: &StoragePlan) -> Result<(), BillingError> {
        validate_key(&plan.id)?;
        let quota = i64::try_from(plan.storage_quota_bytes).ok().filter(|quota| *quota > 0).ok_or(BillingError::InvalidInput("plan quota must be positive and fit a signed 64-bit integer"))?;
        let stored: i64 = sqlx::query_scalar(
            "INSERT INTO billing_plan(id, storage_quota_bytes) VALUES ($1, $2)
             ON CONFLICT (id) DO UPDATE SET id=EXCLUDED.id RETURNING storage_quota_bytes",
        )
        .bind(&plan.id)
        .bind(quota)
        .fetch_one(self.database.pool())
        .await?;
        if stored != quota {
            return Err(BillingError::PlanConflict);
        }
        Ok(())
    }

    /// Server-only state for a future provider adapter. Missing means revision 0.
    pub async fn subscription(&self, user_id: &str) -> Result<Option<StoredSubscription>, BillingError> {
        let row = sqlx::query("SELECT * FROM billing_subscription WHERE user_id=$1").bind(user_id).fetch_optional(self.database.pool()).await?;
        row.map(|row| {
            Ok(StoredSubscription {
                revision: row.try_get("revision")?,
                snapshot: SubscriptionSnapshot {
                    user_id: user_id.to_owned(),
                    provider: row.try_get("provider")?,
                    customer_id: row.try_get("customer_id")?,
                    subscription_id: row.try_get("subscription_id")?,
                    plan_id: row.try_get("plan_id")?,
                    pending_plan_change: row.try_get::<Option<String>, _>("pending_plan_id")?.map(|plan_id| Ok::<_, SqlxError>(PendingPlanChange { plan_id, effective_at: row.try_get("pending_change_at")? })).transpose()?,
                    status: parse_status(row.try_get("status")?)?,
                    current_period_end: row.try_get("current_period_end")?,
                    cancel_at_period_end: row.try_get("cancel_at_period_end")?,
                    cancel_effective_at: row.try_get("cancel_effective_at")?,
                    access_until: row.try_get("access_until")?,
                },
            })
        })
        .transpose()
    }

    /// Ordinary cancellation/downgrade preserves the current paid period.
    /// This records an acknowledged provider change; it does not contact a
    /// provider or promise another paid period without a renewal access grant.
    pub async fn schedule_change(&self, request: &SubscriptionChangeRequest) -> Result<ApplySubscriptionResult, BillingError> {
        validate_reference(&request.event_id)?;
        validate_key(&request.provider)?;
        validate_reference(&request.subscription_id)?;
        // Retries are still successful after the period boundary or a renewal.
        if let Some(receipt) = sqlx::query("SELECT user_id, fingerprint, applied_revision FROM billing_event WHERE provider=$1 AND event_id=$2").bind(&request.provider).bind(&request.event_id).fetch_optional(self.database.pool()).await? {
            if receipt.try_get::<String, _>("user_id")? != request.user_id || receipt.try_get::<Vec<u8>, _>("fingerprint")?.as_slice() != request.fingerprint.as_slice() {
                return Err(BillingError::EventConflict);
            }
            return Ok(ApplySubscriptionResult::Duplicate { revision: receipt.try_get("applied_revision")? });
        }
        let stored = self.subscription(&request.user_id).await?.ok_or(BillingError::InvalidInput("account has no subscription to change"))?;
        let mut snapshot = stored.snapshot;
        if snapshot.provider != request.provider || snapshot.subscription_id != request.subscription_id {
            return Err(BillingError::InvalidInput("change targets a different provider or subscription"));
        }
        if stored.revision != request.expected_revision {
            return Err(BillingError::RevisionConflict { actual_revision: stored.revision });
        }
        let current = sqlx::query(
            "SELECT plan_id, cancel_effective_at, pending_change_at,
                    COALESCE(access_active AND current_period_end>EXTRACT(EPOCH FROM statement_timestamp()), false) AS open_period
             FROM billing_subscription_entitlement WHERE user_id=$1",
        )
        .bind(&request.user_id)
        .fetch_one(self.database.pool())
        .await?;
        if !current.try_get::<bool, _>("open_period")? {
            return Err(BillingError::InvalidInput("subscription has no unexpired paid period to change"));
        }
        // An existing future change fixes the boundary even if renewal metadata
        // has already advanced the period end. The projection hides completed
        // downgrades, so they cannot supply an obsolete date for a new change.
        let effective_at = current.try_get::<Option<i64>, _>("cancel_effective_at")?.or(current.try_get("pending_change_at")?).or(snapshot.current_period_end).ok_or(BillingError::InvalidInput("subscription has no billing period end"))?;
        // A previous downgrade may already be effective without a new webhook.
        // Clearing that schedule must keep the current plan, not restore its
        // historical larger predecessor.
        snapshot.plan_id = current.try_get("plan_id")?;
        snapshot.pending_plan_change = None;
        snapshot.cancel_at_period_end = false;
        snapshot.cancel_effective_at = None;
        match &request.change {
            SubscriptionChange::CancelAtPeriodEnd => {
                snapshot.cancel_at_period_end = true;
                snapshot.cancel_effective_at = Some(effective_at);
            }
            SubscriptionChange::Downgrade { plan_id } => snapshot.pending_plan_change = Some(PendingPlanChange { plan_id: plan_id.clone(), effective_at }),
            SubscriptionChange::KeepCurrentPlan => {}
        }
        self.apply_subscription(&SubscriptionEvent { event_id: request.event_id.clone(), fingerprint: request.fingerprint, expected_revision: request.expected_revision, snapshot }).await
    }

    pub async fn entitlements(&self, user_id: &str) -> Result<AccountEntitlementsResponse, BillingError> {
        let mut transaction = self.database.pool().begin().await?;
        ensure_account(&mut transaction, user_id).await?;
        let row = sqlx::query(
            "SELECT account.quota_bytes AS base_quota,
                    billing_storage_quota(account.user_id, account.quota_bytes) AS effective_quota,
                    subscription.plan_id, subscription.storage_quota_bytes AS plan_quota,
                    subscription.status, subscription.current_period_end,
                    subscription.cancel_at_period_end, subscription.cancel_effective_at, subscription.access_until,
                    subscription.access_active, subscription.pending_plan_id,
                    subscription.pending_plan_quota, subscription.pending_change_at
             FROM user_storage_account account
             LEFT JOIN billing_subscription_entitlement subscription ON subscription.user_id=account.user_id
             WHERE account.user_id=$1",
        )
        .bind(user_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(BillingError::UnknownAccount)?;
        let subscription = if let Some(id) = row.try_get::<Option<String>, _>("plan_id")? {
            Some(AccountSubscription {
                plan: StoragePlan { id, storage_quota_bytes: row.try_get::<i64, _>("plan_quota")? as u64 },
                pending_plan_change: row
                    .try_get::<Option<String>, _>("pending_plan_id")?
                    .map(|id| Ok::<_, SqlxError>(ScheduledStoragePlanChange { plan: StoragePlan { id, storage_quota_bytes: row.try_get::<i64, _>("pending_plan_quota")? as u64 }, effective_at: row.try_get("pending_change_at")? }))
                    .transpose()?,
                status: parse_status(row.try_get("status")?)?,
                current_period_end: row.try_get("current_period_end")?,
                cancel_at_period_end: row.try_get("cancel_at_period_end")?,
                cancel_effective_at: row.try_get("cancel_effective_at")?,
                access_until: row.try_get("access_until")?,
                access_active: row.try_get("access_active")?,
            })
        } else {
            None
        };
        let response = AccountEntitlementsResponse { base_storage_quota_bytes: row.try_get::<i64, _>("base_quota")? as u64, storage_quota_bytes: row.try_get::<i64, _>("effective_quota")? as u64, subscription };
        transaction.commit().await?;
        Ok(response)
    }

    /// Atomically applies a verified, freshly fetched snapshot and records its
    /// event receipt. There is deliberately no public HTTP mutation endpoint.
    pub async fn apply_subscription(&self, event: &SubscriptionEvent) -> Result<ApplySubscriptionResult, BillingError> {
        let snapshot = &event.snapshot;
        validate_snapshot(snapshot)?;
        validate_reference(&event.event_id)?;
        let next_revision = event.expected_revision.checked_add(1).filter(|_| event.expected_revision >= 0).ok_or(BillingError::InvalidInput("invalid subscription revision"))?;
        let payload = serde_json::to_value(snapshot)?;
        let mut transaction = self.database.pool().begin().await?;
        // Retirement must be observed by a fresh statement after the ownership
        // write, including when the database default isolation is stricter.
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED").execute(&mut *transaction).await?;
        // Attachment and orphan cleanup share this lock even when no ownership
        // row exists. A late event cannot queue cancellation while another
        // account is attaching the same external subscription.
        lock_external_subscription(&mut transaction, &snapshot.provider, &snapshot.subscription_id).await?;
        // Prevent account deletion from passing between validation and the
        // cancellation trigger. Always lock identity before the quota ledger.
        let exists = sqlx::query("SELECT id FROM users WHERE id=$1 FOR KEY SHARE").bind(&snapshot.user_id).fetch_optional(&mut *transaction).await?.is_some();
        if !exists {
            let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM billing_subscription WHERE provider=$1 AND subscription_id=$2)").bind(&snapshot.provider).bind(&snapshot.subscription_id).fetch_one(&mut *transaction).await?;
            if owned {
                return Err(BillingError::SubscriptionOwnedByLiveAccount);
            }
            // Account deletion may have happened before any local subscription
            // existed. Its trigger then had nothing to queue. This outbox has
            // no user FK and deduplicates retries by external subscription ID.
            queue_cancellation(&mut transaction, &snapshot.provider, &snapshot.customer_id, &snapshot.subscription_id).await?;
            transaction.commit().await?;
            return Ok(ApplySubscriptionResult::OrphanCancellationRecorded);
        }
        ensure_account(&mut transaction, &snapshot.user_id).await?;
        sqlx::query("SELECT user_id FROM user_storage_account WHERE user_id=$1 FOR UPDATE").bind(&snapshot.user_id).fetch_one(&mut *transaction).await?;

        if let Some(receipt) = sqlx::query("SELECT user_id, fingerprint, applied_revision FROM billing_event WHERE provider=$1 AND event_id=$2").bind(&snapshot.provider).bind(&event.event_id).fetch_optional(&mut *transaction).await? {
            if receipt.try_get::<String, _>("user_id")? != snapshot.user_id || receipt.try_get::<Vec<u8>, _>("fingerprint")?.as_slice() != event.fingerprint.as_slice() {
                return Err(BillingError::EventConflict);
            }
            return Ok(ApplySubscriptionResult::Duplicate { revision: receipt.try_get("applied_revision")? });
        }
        let previous = sqlx::query(
            "SELECT subscription.revision, subscription.provider, subscription.customer_id, subscription.subscription_id,
                    LEAST(subscription.current_period_end, entitlement.pending_change_at,
                        CASE WHEN entitlement.cancel_at_period_end THEN entitlement.cancel_effective_at END) AS protected_period_end,
                    entitlement.storage_quota_bytes AS effective_quota,
                    COALESCE(entitlement.access_active AND entitlement.current_period_end>EXTRACT(EPOCH FROM statement_timestamp()), false) AS paid_period_open,
                    billing_access_active($2, $3) AS incoming_access_active
             FROM billing_subscription subscription
             JOIN billing_subscription_entitlement entitlement ON entitlement.user_id=subscription.user_id
             WHERE subscription.user_id=$1",
        )
        .bind(&snapshot.user_id)
        .bind(status_name(snapshot.status))
        .bind(snapshot.access_until)
        .fetch_optional(&mut *transaction)
        .await?;
        let actual_revision = previous.as_ref().map(|row| row.try_get::<i64, _>("revision")).transpose()?.unwrap_or(0);
        if actual_revision != event.expected_revision {
            return Err(BillingError::RevisionConflict { actual_revision });
        }
        let plan_quota: i64 = sqlx::query_scalar("SELECT storage_quota_bytes FROM billing_plan WHERE id=$1").bind(&snapshot.plan_id).fetch_optional(&mut *transaction).await?.ok_or(BillingError::UnknownPlan)?;
        if let Some(pending) = &snapshot.pending_plan_change {
            let pending_quota: i64 = sqlx::query_scalar("SELECT storage_quota_bytes FROM billing_plan WHERE id=$1").bind(&pending.plan_id).fetch_optional(&mut *transaction).await?.ok_or(BillingError::UnknownPlan)?;
            if pending_quota >= plan_quota {
                return Err(BillingError::InvalidInput("scheduled downgrade must have a smaller storage allowance"));
            }
        }
        if let Some(previous) = &previous {
            let same_subscription = previous.try_get::<String, _>("provider")? == snapshot.provider && previous.try_get::<String, _>("subscription_id")? == snapshot.subscription_id;
            if same_subscription && previous.try_get::<bool, _>("paid_period_open")? && previous.try_get::<bool, _>("incoming_access_active")? {
                if plan_quota < previous.try_get::<i64, _>("effective_quota")? {
                    return Err(BillingError::InvalidInput("schedule a downgrade for the end of the current paid period"));
                }
                // A pending change already ends the current plan's paid period.
                // Its target/action may change without postponing that boundary.
                let period_end: i64 = previous.try_get("protected_period_end")?;
                if snapshot.cancel_effective_at.is_some_and(|at| at < period_end) {
                    return Err(BillingError::InvalidInput("cancellation must preserve the current paid period"));
                }
                if let Some(pending) = &snapshot.pending_plan_change {
                    if pending.effective_at < period_end {
                        return Err(BillingError::InvalidInput("a new downgrade must preserve the current paid period"));
                    }
                }
            }
        }
        if let Some(previous) = &previous {
            let provider: String = previous.try_get("provider")?;
            let subscription: String = previous.try_get("subscription_id")?;
            if provider != snapshot.provider || subscription != snapshot.subscription_id {
                queue_cancellation(&mut transaction, &provider, &previous.try_get::<String, _>("customer_id")?, &subscription).await?;
            }
        }
        sqlx::query(
            "INSERT INTO billing_subscription(user_id, provider, customer_id, subscription_id, plan_id, status,
                 current_period_end, cancel_at_period_end, access_until, revision, pending_plan_id, pending_change_at, cancel_effective_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
             ON CONFLICT (user_id) DO UPDATE SET provider=EXCLUDED.provider, customer_id=EXCLUDED.customer_id,
                 subscription_id=EXCLUDED.subscription_id, plan_id=EXCLUDED.plan_id, status=EXCLUDED.status,
                 current_period_end=EXCLUDED.current_period_end, cancel_at_period_end=EXCLUDED.cancel_at_period_end,
                 access_until=EXCLUDED.access_until, revision=EXCLUDED.revision,
                 pending_plan_id=EXCLUDED.pending_plan_id, pending_change_at=EXCLUDED.pending_change_at,
                 cancel_effective_at=EXCLUDED.cancel_effective_at, updated_at=now()",
        )
        .bind(&snapshot.user_id)
        .bind(&snapshot.provider)
        .bind(&snapshot.customer_id)
        .bind(&snapshot.subscription_id)
        .bind(&snapshot.plan_id)
        .bind(status_name(snapshot.status))
        .bind(snapshot.current_period_end)
        .bind(snapshot.cancel_at_period_end)
        .bind(snapshot.access_until)
        .bind(next_revision)
        .bind(snapshot.pending_plan_change.as_ref().map(|pending| pending.plan_id.as_str()))
        .bind(snapshot.pending_plan_change.as_ref().map(|pending| pending.effective_at))
        .bind(snapshot.cancel_effective_at)
        .execute(&mut *transaction)
        .await?;
        // The unique (provider, subscription_id) ownership write above waits for
        // any concurrent deletion/replacement of the former owner to commit.
        // Check retirement AFTER that wait: its cancellation job is now visible
        // in this fresh READ COMMITTED statement. Rejecting rolls back both the
        // ownership write and any replacement cancellation queued above.
        let canceling: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM billing_cancellation_outbox WHERE provider=$1 AND subscription_id=$2)").bind(&snapshot.provider).bind(&snapshot.subscription_id).fetch_one(&mut *transaction).await?;
        if canceling {
            return Err(BillingError::CancellationPending);
        }
        if let Some(previous) = &previous {
            let replacing = previous.try_get::<String, _>("provider")? != snapshot.provider || previous.try_get::<String, _>("subscription_id")? != snapshot.subscription_id;
            if replacing && previous.try_get::<bool, _>("paid_period_open")? {
                // Read the actual projection rather than the raw snapshot:
                // pending changes and cancellation can lower access now or
                // before the previous paid boundary, despite a larger plan ID.
                let proposed = sqlx::query(
                    "SELECT storage_quota_bytes, access_active, access_until, current_period_end,
                            pending_change_at, pending_plan_quota
                     FROM billing_subscription_entitlement WHERE user_id=$1",
                )
                .bind(&snapshot.user_id)
                .fetch_one(&mut *transaction)
                .await?;
                let until: i64 = previous.try_get("protected_period_end")?;
                let quota: i64 = previous.try_get("effective_quota")?;
                let pending_reduction = match (proposed.try_get::<Option<i64>, _>("pending_change_at")?, proposed.try_get::<Option<i64>, _>("pending_plan_quota")?) {
                    (Some(at), Some(pending_quota)) => at < until && pending_quota < quota,
                    _ => false,
                };
                if !proposed.try_get::<bool, _>("access_active")?
                    || proposed.try_get::<i64, _>("storage_quota_bytes")? < quota
                    || proposed.try_get::<Option<i64>, _>("access_until")?.unwrap_or(0) < until
                    || proposed.try_get::<Option<i64>, _>("current_period_end")?.unwrap_or(0) < until
                    || pending_reduction
                {
                    // This also rolls back the old subscription's queued
                    // cancellation, leaving its paid allowance unchanged.
                    return Err(BillingError::PaidPeriodProtected { until });
                }
            }
        }
        sqlx::query("INSERT INTO billing_event(provider, event_id, user_id, fingerprint, snapshot, applied_revision) VALUES ($1,$2,$3,$4,$5,$6)")
            .bind(&snapshot.provider)
            .bind(&event.event_id)
            .bind(&snapshot.user_id)
            .bind(event.fingerprint.as_slice())
            .bind(payload)
            .bind(next_revision)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(ApplySubscriptionResult::Applied { revision: next_revision })
    }

    /// Claim only jobs for this adapter. A crashed worker's five-minute lease
    /// expires automatically; external cancellation must itself be idempotent.
    pub async fn claim_cancellations(&self, provider: &str, limit: u32) -> Result<Vec<CancellationJob>, BillingError> {
        validate_key(provider)?;
        if !(1..=100).contains(&limit) {
            return Err(BillingError::InvalidInput("cancellation batch size must be between 1 and 100"));
        }
        let token = Uuid::new_v4();
        let rows = sqlx::query(
            "WITH pending AS (
                 SELECT id FROM billing_cancellation_outbox
                 WHERE provider=$1 AND completed_at IS NULL AND available_at<=now() AND (lease_until IS NULL OR lease_until<=now())
                 ORDER BY id LIMIT $2 FOR UPDATE SKIP LOCKED
             )
             UPDATE billing_cancellation_outbox job
             SET lease_token=$3, lease_until=now()+interval '5 minutes', attempts=attempts+1
             FROM pending WHERE job.id=pending.id
             RETURNING job.id, job.provider, job.customer_id, job.subscription_id",
        )
        .bind(provider)
        .bind(i64::from(limit))
        .bind(token)
        .fetch_all(self.database.pool())
        .await?;
        rows.into_iter().map(|row| Ok(CancellationJob { id: row.try_get("id")?, lease_token: token, provider: row.try_get("provider")?, customer_id: row.try_get("customer_id")?, subscription_id: row.try_get("subscription_id")? })).collect()
    }

    /// Returns false if the lease expired or another worker reclaimed the job.
    pub async fn complete_cancellation(&self, job: &CancellationJob) -> Result<bool, BillingError> {
        // Keep a tombstone so delayed events cannot reattach a retired external
        // subscription after its cancellation job has been completed.
        Ok(sqlx::query("UPDATE billing_cancellation_outbox SET completed_at=now(), lease_token=NULL, lease_until=NULL WHERE id=$1 AND lease_token=$2 AND lease_until>now() AND completed_at IS NULL")
            .bind(job.id)
            .bind(job.lease_token)
            .execute(self.database.pool())
            .await?
            .rows_affected()
            == 1)
    }

    pub async fn retry_cancellation(&self, job: &CancellationJob) -> Result<bool, BillingError> {
        Ok(sqlx::query(
            "UPDATE billing_cancellation_outbox SET lease_token=NULL, lease_until=NULL,
                 available_at=now()+interval '1 minute' * LEAST(attempts, 60)
             WHERE id=$1 AND lease_token=$2 AND lease_until>now()",
        )
        .bind(job.id)
        .bind(job.lease_token)
        .execute(self.database.pool())
        .await?
        .rows_affected()
            == 1)
    }
}

async fn ensure_account(transaction: &mut Transaction<'_, Postgres>, user_id: &str) -> Result<(), SqlxError> {
    sqlx::query(include_str!("sql/account/ensure_exists.sql")).bind(user_id).execute(&mut **transaction).await?;
    Ok(())
}

async fn lock_external_subscription(transaction: &mut Transaction<'_, Postgres>, provider: &str, subscription_id: &str) -> Result<(), SqlxError> {
    // Providers cannot contain ':', so the namespaced reference is unambiguous.
    let reference = format!("bokheim-billing-subscription:{provider}:{subscription_id}");
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))").bind(reference).execute(&mut **transaction).await?;
    Ok(())
}

async fn queue_cancellation(transaction: &mut Transaction<'_, Postgres>, provider: &str, customer_id: &str, subscription_id: &str) -> Result<(), SqlxError> {
    sqlx::query(
        "INSERT INTO billing_cancellation_outbox(provider, customer_id, subscription_id) VALUES ($1, $2, $3)
         ON CONFLICT (provider, subscription_id) DO NOTHING",
    )
    .bind(provider)
    .bind(customer_id)
    .bind(subscription_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn validate_key(value: &str) -> Result<(), BillingError> {
    if value.is_empty() || value.len() > 64 || !value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')) {
        return Err(BillingError::InvalidInput("billing keys must contain 1–64 lowercase ASCII letters, digits, underscores or hyphens"));
    }
    Ok(())
}

fn validate_reference(value: &str) -> Result<(), BillingError> {
    if value.is_empty() || value.len() > 255 || value.chars().any(char::is_control) {
        return Err(BillingError::InvalidInput("billing references must contain 1–255 bytes without control characters"));
    }
    Ok(())
}

fn validate_snapshot(snapshot: &SubscriptionSnapshot) -> Result<(), BillingError> {
    validate_key(&snapshot.provider)?;
    validate_key(&snapshot.plan_id)?;
    validate_reference(&snapshot.user_id)?;
    validate_reference(&snapshot.customer_id)?;
    validate_reference(&snapshot.subscription_id)?;
    if snapshot.current_period_end.is_some_and(|value| value <= 0) || snapshot.access_until.is_some_and(|value| value <= 0) {
        return Err(BillingError::InvalidInput("subscription deadlines must be positive Unix timestamps"));
    }
    if matches!(snapshot.status, SubscriptionStatus::Pending | SubscriptionStatus::Paused | SubscriptionStatus::Expired) && snapshot.access_until.is_some() {
        return Err(BillingError::InvalidInput("non-entitled subscription states must not grant access"));
    }
    if snapshot.cancel_at_period_end != snapshot.cancel_effective_at.is_some() || snapshot.cancel_effective_at.is_some_and(|at| at <= 0 || snapshot.current_period_end.is_none_or(|end| at > end)) {
        return Err(BillingError::InvalidInput("scheduled cancellation requires a fixed cutoff at or before the billing period end"));
    }
    if let Some(pending) = &snapshot.pending_plan_change {
        validate_key(&pending.plan_id)?;
        if pending.effective_at <= 0 || snapshot.current_period_end.is_none_or(|end| pending.effective_at > end) || pending.plan_id == snapshot.plan_id || snapshot.cancel_at_period_end {
            return Err(BillingError::InvalidInput("a pending downgrade must replace the plan at the billing period end and cannot coexist with cancellation"));
        }
    }
    Ok(())
}

fn status_name(status: SubscriptionStatus) -> &'static str {
    match status {
        SubscriptionStatus::Pending => "pending",
        SubscriptionStatus::Trialing => "trialing",
        SubscriptionStatus::Active => "active",
        SubscriptionStatus::PastDue => "past_due",
        SubscriptionStatus::Paused => "paused",
        SubscriptionStatus::Canceled => "canceled",
        SubscriptionStatus::Expired => "expired",
    }
}

fn parse_status(status: &str) -> Result<SubscriptionStatus, BillingError> {
    match status {
        "pending" => Ok(SubscriptionStatus::Pending),
        "trialing" => Ok(SubscriptionStatus::Trialing),
        "active" => Ok(SubscriptionStatus::Active),
        "past_due" => Ok(SubscriptionStatus::PastDue),
        "paused" => Ok(SubscriptionStatus::Paused),
        "canceled" => Ok(SubscriptionStatus::Canceled),
        "expired" => Ok(SubscriptionStatus::Expired),
        _ => Err(BillingError::InvalidInput("unknown stored subscription status")),
    }
}
