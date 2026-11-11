//! Account operations can be awaited from native UI executors and browser event loops.
//! Native HTTP runtime integration is handled internally.
mod account;
mod auth;
mod authenticated_request;
mod http;
mod lifecycle;
mod session;
mod status;

pub use account::{
    account_entitlements, account_snapshot, book_storage_usage, cloud_storage, create_library, create_library_with_storage, current_account, delete_library, list_account_sessions, rename_library, revoke_account_session,
    revoke_other_account_sessions, set_cloud_storage,
};
pub use account_contract::{
    AccountEntitlementsResponse, AccountSnapshotResponse, AccountSubscription, LibraryStorageUsage, OtherSessionsRevocationResponse, ScheduledStoragePlanChange, SessionRevocationResponse, SessionSummary, StoragePlan, StorageUsageResponse,
    SubscriptionStatus,
};
pub use auth::{delete_account, login, logout, refresh_session, request_password_reset, request_public_registration, resend_verification, reset_password, verify_email};
pub use lifecycle::{PendingSessionChange, SessionChange};
pub use session::{validate_user_id, AuthError, Session};
pub use status::AccountStatus;
pub use url::Url;

pub const API_SERVER_ORIGIN: &str = "https://api.bokheim.se";

pub fn api_server_url() -> Url {
    Url::parse(API_SERVER_ORIGIN).expect("Bokheim API origin is a valid URL")
}

mod session_manager;
pub use authenticated_request::{account_request, authenticated_request, AuthenticatedRequestError, RefreshPolicy, SessionAuthority};
pub use session_manager::{AccountSession, AccountSessionError, SessionFuture, SessionPersistence};
