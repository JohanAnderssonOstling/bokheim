//! Portable account HTTP contracts shared by clients and account transports.

use serde::{Deserialize, Serialize};

pub const PASSWORD_MIN_CHARACTERS: usize = 12;
pub const PASSWORD_MAX_BYTES: usize = 1024;
pub const EMAIL_MAX_BYTES: usize = 254;
pub const EMAIL_VERIFICATION_PIN_DIGITS: usize = 6;
pub const ONE_TIME_TOKEN_BYTES: usize = 43;

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicRegistrationRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub struct EmailRequest {
    pub email: String,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub struct EmailVerificationRequest {
    pub email: String,
    pub pin: String,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub struct ResetPasswordRequest {
    pub token: String,
    pub new_password: String,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthResponse {
    /// Short-lived bearer credential used for ordinary API requests.
    pub token: String,
    /// Absolute Unix timestamp after which the access token is invalid.
    pub expires_at: i64,
    /// Long-lived credential accepted only by the refresh endpoint.
    pub refresh_token: String,
    /// Absolute Unix timestamp after which the refresh token is invalid.
    pub refresh_expires_at: i64,
    pub user_id: String,
    pub email: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MeResponse {
    pub user_id: String,
    pub email: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct StorageUsageResponse {
    /// Bytes held by completed, user-owned book and thumbnail assets.
    pub used_bytes: u64,
    /// Bytes temporarily reserved for uploads that have not completed yet.
    pub reserved_bytes: u64,
    pub quota_bytes: u64,
}

impl StorageUsageResponse {
    pub const fn available_bytes(self) -> u64 {
        self.quota_bytes.saturating_sub(self.used_bytes.saturating_add(self.reserved_bytes))
    }
}

/// One library's share of [`StorageUsageResponse::used_bytes`].
///
/// A separate response rather than a field on `StorageUsageResponse`: the
/// storage endpoint is already deployed, and adding a field to its encoding
/// would break clients and servers that disagree about the version. Clients
/// treat a missing route the same way they already treat a server with no quota
/// endpoint at all.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryStorageUsage {
    pub library_id: String,
    /// Bytes charged to the account and attributed to this library. The
    /// attributions across libraries partition the account's used bytes, so a
    /// book held by two libraries is counted under one of them only. A thumbnail
    /// is attributed to the first library that uploads it.
    pub used_bytes: u64,
}

/// Everything an account settings screen needs in one server response.
/// Device-local storage remains a client concern.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountSnapshotResponse {
    pub libraries: Vec<sync_common::api::libraries::LibrarySummary>,
    pub deleted_library_ids: Vec<sync_common::LibraryId>,
    pub storage: StorageUsageResponse,
    pub library_storage: Vec<LibraryStorageUsage>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionSummary {
    pub session_id: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub current: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionRevocationResponse {
    pub revoked: bool,
    pub signed_out: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct OtherSessionsRevocationResponse {
    pub revoked: u64,
}
