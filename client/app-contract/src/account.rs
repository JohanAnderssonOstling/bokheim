use serde::{Deserialize, Serialize};
use specta::Type;
use sync_common::LibraryId;

#[derive(Clone, Debug, Default, Deserialize, Serialize, Type)]
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, Type)]
pub struct AccountStorageUsage {
    #[specta(type = f64)]
    pub used_bytes: u64,
    #[specta(type = f64)]
    pub reserved_bytes: u64,
    #[specta(type = f64)]
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

#[cfg(test)]
mod tests {
    use super::AccountStorageUsage;

    #[test]
    fn available_storage_accounts_for_active_reservations() {
        assert_eq!(AccountStorageUsage { used_bytes: 60, reserved_bytes: 15, quota_bytes: 100 }.available_bytes(), 25);
        assert_eq!(AccountStorageUsage { used_bytes: 120, reserved_bytes: 0, quota_bytes: 100 }.available_bytes(), 0);
    }
}

#[cfg(test)]
mod summary_tests {
    use super::{LibrarySyncState, LibrarySyncStatus, LibrarySyncSummary};
    use sync_common::LibraryId;

    /// The fixtures mirror `apps/mobile/src/lib/sync-summary.test.ts`, which
    /// checks the same rule in the mobile frontend.
    fn status(state: LibrarySyncState, has_unsynced_changes: bool, last_synced_at: Option<u64>) -> LibrarySyncStatus {
        LibrarySyncStatus { library_id: LibraryId::nil(), library_name: "Library".to_owned(), state, server_url: None, account_id: None, has_unsynced_changes, last_synced_at }
    }

    #[test]
    fn a_settled_library_needs_no_attention() {
        assert!(!status(LibrarySyncState::Synced, false, Some(10)).needs_attention());
        assert!(status(LibrarySyncState::Synced, true, Some(10)).needs_attention());
        assert!(status(LibrarySyncState::Synced, false, None).needs_attention());
        assert!(status(LibrarySyncState::LocalOnly, false, Some(10)).needs_attention());
        assert!(status(LibrarySyncState::RemoteMissing, false, Some(10)).needs_attention());
    }

    #[test]
    fn the_summary_counts_libraries_and_keeps_the_latest_sync() {
        let statuses = [status(LibrarySyncState::Synced, false, Some(30)), status(LibrarySyncState::Synced, false, Some(70)), status(LibrarySyncState::LocalOnly, false, Some(50))];
        let summary = LibrarySyncSummary::from_statuses(&statuses);
        assert_eq!(summary.library_count, 3);
        assert_eq!(summary.attention_count, 1);
        assert_eq!(summary.last_synced_at, Some(70));
        assert!(!summary.all_synced());
    }

    #[test]
    fn everything_synced_requires_at_least_one_library() {
        assert!(!LibrarySyncSummary::from_statuses(&[]).all_synced());
        assert!(LibrarySyncSummary::from_statuses(&[status(LibrarySyncState::Synced, false, Some(1))]).all_synced());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySyncState {
    LocalOnly,
    Synced,
    SignInRequired,
    DifferentAccount,
    RemoteMissing,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
pub struct LibrarySyncStatus {
    #[specta(type = String)]
    pub library_id: LibraryId,
    pub library_name: String,
    pub state: LibrarySyncState,
    pub server_url: Option<String>,
    pub account_id: Option<String>,
    /// Whether this device has local metadata changes waiting to be accepted by
    /// the synchronization server.
    pub has_unsynced_changes: bool,
    /// Unix timestamp in milliseconds for the latest successful synchronization
    /// completed by this device.
    #[specta(type = Option<specta_typescript::Number>)]
    pub last_synced_at: Option<u64>,
}

impl LibrarySyncStatus {
    /// Whether this library is anything other than fully mirrored on the
    /// account. A library that reports [`LibrarySyncState::Synced`] but has
    /// never completed a synchronization, or is holding local edits, is not
    /// settled either — the account page must not claim it is.
    pub const fn needs_attention(&self) -> bool {
        !matches!(self.state, LibrarySyncState::Synced) || self.has_unsynced_changes || self.last_synced_at.is_none()
    }
}

/// The single verdict the account page opens with.
///
/// Both frontends answer "is everything synced?" from this rule so the headline
/// cannot disagree between desktop and mobile. Wording lives in
/// `AccountContract`; relative time formatting stays with each frontend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, Type)]
pub struct LibrarySyncSummary {
    pub library_count: u32,
    pub attention_count: u32,
    /// The most recent successful synchronization across every library.
    #[specta(type = Option<specta_typescript::Number>)]
    pub last_synced_at: Option<u64>,
}

impl LibrarySyncSummary {
    pub fn from_statuses(statuses: &[LibrarySyncStatus]) -> Self {
        Self { library_count: statuses.len() as u32, attention_count: statuses.iter().filter(|status| status.needs_attention()).count() as u32, last_synced_at: statuses.iter().filter_map(|status| status.last_synced_at).max() }
    }

    /// Deliberately false when there are no libraries: an empty device has
    /// nothing to be synced, and claiming success would be a lie of omission.
    pub const fn all_synced(&self) -> bool {
        self.library_count > 0 && self.attention_count == 0
    }
}

/// One library's share of the account's charged bytes, as reported by the
/// server.
///
/// Distinct from [`LocalLibraryStorageUsage`], which measures files downloaded
/// to this device. The account page shows both and must never conflate them:
/// the quota is spent on the server, and a library can occupy space there with
/// nothing downloaded here, or the reverse.
///
/// The shares partition the account's used bytes — a book held by two libraries
/// is charged once and attributed to one of them — so they can be drawn against
/// the quota, but a single library's share understates what that library
/// contains when it shares books with another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, Type)]
pub struct ServerLibraryStorageUsage {
    #[specta(type = String)]
    pub library_id: LibraryId,
    #[specta(type = f64)]
    pub used_bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, Type)]
pub struct LocalLibraryStorageUsage {
    #[specta(type = String)]
    pub library_id: LibraryId,
    pub library_name: String,
    /// Apparent bytes occupied by book files downloaded for offline use.
    /// Imported originals, databases, thumbnails, and shared caches are excluded.
    #[specta(type = f64)]
    pub used_bytes: u64,
}
