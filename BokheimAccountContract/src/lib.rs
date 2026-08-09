//! Portable account HTTP contracts shared by clients and account transports.

use serde::{Deserialize, Serialize};

pub const PASSWORD_MIN_CHARACTERS: usize = 12;
pub const PASSWORD_MAX_BYTES: usize = 1024;
pub const EMAIL_MAX_BYTES: usize = 254;
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
pub struct OneTimeTokenRequest {
    pub token: String,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_and_refresh_credentials_have_distinct_wire_fields() {
        let response = AuthResponse { token: "access".to_owned(), expires_at: 100, refresh_token: "refresh".to_owned(), refresh_expires_at: 200, user_id: "user-1".to_owned(), email: "reader@example.com".to_owned() };
        let encoded = bokheim_wire::encode(&response).unwrap();
        let value: AuthResponse = bokheim_wire::decode(&encoded, 1024).unwrap();
        assert_eq!(value.token, "access");
        assert_eq!(value.refresh_token, "refresh");
    }

    #[test]
    fn session_summaries_expose_identifiers_and_times_but_no_credentials() {
        let summary = SessionSummary { session_id: "session-1".to_owned(), created_at: 100, expires_at: 200, current: true };
        let encoded = bokheim_wire::encode(&summary).unwrap();
        let value: SessionSummary = bokheim_wire::decode(&encoded, 1024).unwrap();
        assert_eq!(value, summary);
    }
}
