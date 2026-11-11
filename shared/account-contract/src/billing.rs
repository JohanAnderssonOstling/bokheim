//! Provider-independent account entitlements. Payment identifiers stay on the server.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    Pending,
    Trialing,
    Active,
    PastDue,
    Paused,
    Canceled,
    Expired,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoragePlan {
    /// Stable Bokheim identifier, independent of a provider's product or price.
    pub id: String,
    pub storage_quota_bytes: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduledStoragePlanChange {
    pub plan: StoragePlan,
    /// Unix-second boundary at which this plan replaces the current plan.
    pub effective_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountSubscription {
    pub plan: StoragePlan,
    /// Present only until the scheduled boundary; `plan` is then the new plan.
    pub pending_plan_change: Option<ScheduledStoragePlanChange>,
    pub status: SubscriptionStatus,
    pub current_period_end: Option<i64>,
    pub cancel_at_period_end: bool,
    /// Fixed cancellation cutoff, independent of later billing-period updates.
    pub cancel_effective_at: Option<i64>,
    /// Exclusive Unix-second deadline for access explicitly granted by the server.
    /// A subscription status alone never grants storage.
    pub access_until: Option<i64>,
    /// Evaluated using the server clock when this response was produced.
    pub access_active: bool,
}

/// Separate from the deployed account snapshot to preserve its binary encoding.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountEntitlementsResponse {
    /// Existing free, grandfathered, or operator-configured allowance.
    pub base_storage_quota_bytes: u64,
    /// The greater of the base allowance and a currently valid subscription.
    pub storage_quota_bytes: u64,
    pub subscription: Option<AccountSubscription>,
}
