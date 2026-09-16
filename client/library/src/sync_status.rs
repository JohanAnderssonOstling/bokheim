//! Presentation state derived from one library's durable synchronization data.

use serde::{Deserialize, Serialize};
use sync_common::LibraryId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySyncState {
    Checking,
    Receiving,
    LocalOnly,
    Synced,
    SyncPending,
    SignInRequired,
    DifferentAccount,
    RemoteMissing,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct LibrarySyncStatus {
    #[serde(default = "default_asset_storage_enabled")]
    pub asset_storage_enabled: bool,
    pub library_id: LibraryId,
    pub library_name: String,
    pub state: LibrarySyncState,
    pub server_url: Option<String>,
    pub account_id: Option<String>,
    pub has_unsynced_changes: bool,
    pub last_synced_at: Option<u64>,
}

const fn default_asset_storage_enabled() -> bool {
    true
}

impl LibrarySyncStatus {
    pub const fn needs_attention(&self) -> bool {
        !matches!(self.state, LibrarySyncState::Synced) || self.has_unsynced_changes || self.last_synced_at.is_none()
    }
}
