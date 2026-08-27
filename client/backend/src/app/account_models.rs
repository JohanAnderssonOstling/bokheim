use serde::{Deserialize, Serialize};

use crate::LibraryId;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AccountStatus {
    signed_in: bool,
    server_url: Option<String>,
    email: Option<String>,
}

impl AccountStatus {
    pub fn signed_out() -> Self {
        Self::default()
    }

    pub fn authenticated(server_url: impl Into<String>, email: impl Into<String>) -> Self {
        Self { signed_in: true, server_url: Some(server_url.into()), email: Some(email.into()) }
    }

    pub const fn signed_in(&self) -> bool {
        self.signed_in
    }

    pub fn server_url(&self) -> Option<&str> {
        self.server_url.as_deref()
    }

    pub fn email(&self) -> Option<&str> {
        self.email.as_deref()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct AccountStorageUsage {
    pub used_bytes: u64,
    pub reserved_bytes: u64,
    pub quota_bytes: u64,
}

impl AccountStorageUsage {
    pub const fn available_bytes(self) -> u64 {
        self.quota_bytes.saturating_sub(self.used_bytes.saturating_add(self.reserved_bytes))
    }
}

impl From<account_contract::StorageUsageResponse> for AccountStorageUsage {
    fn from(value: account_contract::StorageUsageResponse) -> Self {
        Self { used_bytes: value.used_bytes, reserved_bytes: value.reserved_bytes, quota_bytes: value.quota_bytes }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySyncState {
    LocalOnly,
    Synced,
    SignInRequired,
    DifferentAccount,
    RemoteMissing,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LibrarySyncStatus {
    pub library_id: LibraryId,
    pub library_name: String,
    pub state: LibrarySyncState,
    pub server_url: Option<String>,
    pub account_id: Option<String>,
    pub has_unsynced_changes: bool,
    pub last_synced_at: Option<u64>,
}

impl LibrarySyncStatus {
    pub const fn needs_attention(&self) -> bool {
        !matches!(self.state, LibrarySyncState::Synced) || self.has_unsynced_changes || self.last_synced_at.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ServerLibraryStorageUsage {
    pub library_id: LibraryId,
    pub used_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct LocalLibraryStorageUsage {
    pub library_id: LibraryId,
    pub library_name: String,
    pub used_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::AccountStorageUsage;

    #[test]
    fn available_storage_accounts_for_active_reservations() {
        assert_eq!(AccountStorageUsage { used_bytes: 60, reserved_bytes: 15, quota_bytes: 100 }.available_bytes(), 25);
        assert_eq!(AccountStorageUsage { used_bytes: 120, reserved_bytes: 0, quota_bytes: 100 }.available_bytes(), 0);
    }
}
