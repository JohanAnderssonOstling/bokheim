//! Presentation state derived from one library's durable synchronization data.

use serde::{Deserialize, Serialize};
use sync_common::{LibraryId, MutationIssue, ReplicaSeq, StateAddress};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct SyncIssueSummary {
    pub retryable: u64,
    pub rejected: u64,
    pub untransmittable: u64,
}

impl SyncIssueSummary {
    pub const fn has_issues(self) -> bool {
        self.retryable != 0 || self.rejected != 0 || self.untransmittable != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct LibrarySyncIssue {
    pub publisher_seq: ReplicaSeq,
    pub address: StateAddress,
    pub issue: MutationIssue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySyncState {
    Checking,
    Unavailable,
    Receiving,
    LocalOnly,
    Synced,
    SyncPending,
    RemoteMissing,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct LibrarySyncStatus {
    #[serde(default)]
    pub issues: SyncIssueSummary,
    #[serde(default = "default_asset_storage_enabled")]
    pub asset_storage_enabled: bool,
    pub library_id: LibraryId,
    pub state: LibrarySyncState,
    pub has_unsynced_changes: bool,
    pub last_synced_at: Option<u64>,
}

const fn default_asset_storage_enabled() -> bool {
    true
}

impl LibrarySyncStatus {
    pub const fn needs_attention(&self) -> bool {
        !matches!(self.state, LibrarySyncState::Synced) || self.has_unsynced_changes || self.last_synced_at.is_none() || self.issues.has_issues()
    }
    /// Display placeholder for a library the backend reported no status for,
    /// built from library data alone. The caller picks the state: whether an
    /// unknown library is "checking" or "local only" depends on sign-in,
    /// which the library does not know.
    pub fn placeholder(library_id: LibraryId, state: LibrarySyncState) -> Self {
        Self { asset_storage_enabled: default_asset_storage_enabled(), library_id, state, has_unsynced_changes: false, last_synced_at: None, issues: SyncIssueSummary::default() }
    }
}
