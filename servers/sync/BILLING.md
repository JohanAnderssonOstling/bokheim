# Provider-neutral billing

The billing foundation lives in `server-postgres::PostgresBillingService`.
It has no payment SDK, credentials, checkout endpoint, prices, currency, or
provider-specific product identifiers. It does not collect payments. There are
no public endpoints for assigning plans or changing subscription state.

## Plans and existing accounts

`register_plan(StoragePlan)` registers a stable Bokheim plan ID and its storage
allowance in bytes. Repeating the same registration is harmless; changing the
quota under an existing ID is rejected. Use a new ID for a changed allowance.
No paid plans or prices are seeded by the migration.

`user_storage_account.quota_bytes` remains the account's base allowance. The
existing 5 GB default and manually configured allowances are preserved. A
subscription raises the effective quota to the greater of its plan allowance
and this base; it never lowers an account's base allowance.

The shared SQL functions `billing_access_active` and `billing_storage_quota`
calculate access using the database clock. Upload admission and account storage
responses use this effective quota. Access expires even if a renewal webhook
never arrives. Existing upload reservations may complete within their normal
short lifetime; expiration/downgrade does not delete files or block downloads
and removal of stored files. Accounts above their effective quota cannot reserve
additional capacity.

## Client contract

Authenticated `GET /api/account/entitlements` returns the base and effective
storage allowances, plus an optional subscription with its Bokheim plan,
normalized status, billing-period end, scheduled cancellation flag, explicit
cancel cutoff, pending plan/date, access deadline and current access flag.
Deadlines are Unix seconds; access ends
at the deadline. Provider customer IDs and subscription IDs never reach clients.

The endpoint uses the existing binary/JSON content negotiation. Existing account
snapshot and storage response encodings are unchanged. The account client adds
`account_entitlements`; a missing endpoint returns `None`, not a fabricated free
subscription. Existing callers can continue using account storage snapshots.

## Scheduled cancellations and downgrades

`schedule_change(SubscriptionChangeRequest)` records an ordinary cancellation,
downgrade, or withdrawal of a scheduled change. It is a trusted server API for
use after a provider confirms the corresponding action, not a public client
endpoint. Requests identify the account, provider and current subscription and
use the same revision checks and source-event receipts as subscription updates.

- `CancelAtPeriodEnd` records a fixed `cancel_effective_at` at the current period
  end. The effective access deadline is capped there, even if a later snapshot
  extends `access_until` or advances the billing-period metadata. At the cutoff,
  the entitlement reports canceled status and falls back to the base allowance.
- `Downgrade` keeps the current plan and stores the smaller plan with a fixed
  effective date at the current period end. The entitlement uses the new plan
  at that date; no scheduled worker or webhook is needed for the switch.
- `KeepCurrentPlan` withdraws either scheduled action before the paid period
  ends. A downgrade that has already taken effect becomes the current plan;
  clearing its old schedule cannot restore the historical larger allowance.

Repeating a scheduled action, changing its target tier, or switching between
cancellation and downgrade preserves the existing future effective date, even
when renewal metadata has advanced the period end. A completed downgrade's old
date is not reused for a new change; withdraw a pending change explicitly with
`KeepCurrentPlan` before scheduling against a later period.

Pending downgrades must reference an existing, strictly smaller plan and cannot
coexist with scheduled cancellation. A subscription with no unexpired paid
period cannot schedule or withdraw changes. Incoming snapshots cannot introduce
a smaller current plan or an earlier scheduled cutoff during an existing paid
period while continuing paid access. Immediate revocation remains available
through a snapshot clearing access, for example after a confirmed refund.

The `billing_subscription_entitlement` view projects the effective plan, status,
pending change, cancellation flag and access deadline from the database clock.
Both upload quotas and account responses use this view. Raw provider snapshots
retain the schedule for reconciliation; merely crossing a date does not mutate
their revision. Adapters must preserve fixed change dates when advancing period
metadata, or materialize an already-effective pending plan as the current plan.

A schedule never grants a new paid period. For example, a 200 GB subscription
scheduled to become 50 GB on December 1 retains 200 GB before that date. From
December 1 it receives 50 GB only while an explicitly confirmed access grant
remains valid; without renewal, it receives the base allowance. Files above the
resulting allowance remain downloadable and removable; additional capacity is
blocked. Already-reserved uploads retain their existing short completion window.

## Future provider adapter

1. Register Bokheim plans and keep provider price-to-plan mappings in the adapter.
2. Authenticate checkout requests with the existing account session and bind
   them to that account on the server. A browser return URL is not payment proof.
3. Verify webhook authenticity before acknowledging or submitting events. If
   acknowledging before application, first persist the event in the adapter's
   durable inbox. The core receipt table is not a webhook delivery queue.
4. Read `subscription(user_id)` to capture its revision (zero if absent), then
   fetch the provider's authoritative current account/subscription state. Check
   that the event belongs to this account and its current subscription. Do not
   interpret an older event payload as current state.
5. Map that state to `SubscriptionSnapshot` and call `apply_subscription` with a
   stable event ID, a 32-byte digest of the authenticated source event (excluding
   delivery headers), and the captured revision. The snapshot and its receipt
   commit atomically. Repeating the source event returns `Duplicate`, even if a
   retry fetched newer provider state. Reusing its ID with a different source
   digest or account is rejected.
6. On `RevisionConflict`, read the revision and fetch provider state again.
   Never relabel a stale snapshot with a newer revision. This prevents concurrent
   workers from overwriting a newer observation. Use the same flow for periodic
   reconciliation, with a unique durable reconciliation-event ID.
7. Define payment, trial, refund, dispute and grace-period policies in the
   adapter. `Active` alone grants nothing: access requires a future
   `access_until`. `Trialing`, `Active`, `PastDue` and `Canceled` can retain
   explicitly granted access; `Pending`, `Paused` and `Expired` cannot. A
   cancellation can retain prepaid access until its explicit deadline.

The core supports one current subscription per account. Do not replace an
existing paid subscription with a pending checkout. A replacement must preserve
at least the existing effective allowance, confirmed access and billing period
through its protected paid boundary, including any already-fixed change date.
An inactive, smaller, shorter-lived or prematurely reducing replacement returns
`PaidPeriodProtected { until }`. Its entire transaction rolls back: the current
subscription and its allowance remain intact, and no cancellation of it is
queued. Use a scheduled change for a downgrade, or wait until the protected
period ends. Adapters should check existing entitlements before creating an
external replacement and handle a rejected external purchase through their
provider's cancellation/refund policy.

An accepted replacement queues cancellation of the former external
subscription. External customer and current subscription IDs cannot be attached
to two accounts.

## Cancellation delivery

A database trigger copies the current external subscription into
`billing_cancellation_outbox` before account deletion cascades remove its billing
state. Replacement subscriptions use the same queue. It contains provider
references, not credentials or account email addresses, and has no foreign key
to the deleted account.

A verified subscription arriving after its account was deleted is also handled:
`apply_subscription` records its cancellation directly in this outbox and commits
`OrphanCancellationRecorded`, granting no access. This covers checkout completing
after deletion, when the deletion trigger had no subscription to copy. Retries
deduplicate on provider/subscription ID, including already-completed cleanup;
they need no event receipt referencing the deleted user.

Attachment and missing-account cleanup acquire the same transaction advisory
lock for the external subscription reference. Cleanup checks that no live
account owns that reference before queuing it; otherwise it returns
`SubscriptionOwnedByLiveAccount`. The lock prevents a live attachment from
slipping between that check and the cancellation insertion. Provider adapters
must verify the source event and its account/customer binding before calling
this trusted API.

An adapter worker uses `claim_cancellations(provider, limit)` to lease at most
100 jobs for five minutes. Cancel externally with an idempotency key derived
from the job ID; treat an already canceled/missing subscription as success.
Call `complete_cancellation` only after external confirmation, or
`retry_cancellation` after a transient failure. Both methods fence writes with
the lease token and return false when the lease is no longer owned. A crashed
worker's lease expires and the job becomes claimable again.

Completed jobs retain a tombstone so delayed events cannot reattach retired
subscription IDs. A canceled/replaced subscription must not be reused for a
new purchase. Attachment checks this tombstone after its unique ownership write
has waited for any former owner's deletion/replacement to commit. A retirement
found there rolls back the attachment and its event receipt. Subscription
updates explicitly use READ COMMITTED isolation so this check sees the
retirement even if the database default isolation is stricter. No worker runs
until a real provider adapter is configured.

## Installation

Fresh databases use `sync-server migrate`, now atomically installing schema 20.
Existing schemas 14, 17, 18 and 19 upgrade automatically on startup before
workers and listeners start. Current databases skip migration after reading
the schema marker. Billing upgrades preserve accounts, base quotas, storage
usage and files; the subsequent book-ID migration rotates sync history so
clients pull the updated identities.

The startup upgrade, also available through `sync-server upgrade`, first runs the legacy
cloud-data upgrade (schema 14 to 17 when needed), then the billing schema upgrade
(17/18 to 19) and the book-ID migration (19 to 20). Take
the normal database backup before upgrading; rolling back to an older server requires
restoring its matching database backup.
