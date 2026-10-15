mod account;
mod auth;
mod http;
mod session;

pub use account::{
    account_snapshot, cloud_storage, create_library, current_account, delete_library, delete_opds_feed, list_account_sessions, opds_feeds, put_opds_feed, rename_library, revoke_account_session, revoke_other_account_sessions,
    set_cloud_storage,
};
pub use account_contract::{AccountSnapshotResponse, LibraryStorageUsage, OtherSessionsRevocationResponse, SessionRevocationResponse, SessionSummary, StorageUsageResponse};
pub use auth::{login, logout, refresh_session, request_password_reset, request_public_registration, resend_verification, reset_password, verify_email};
pub use session::{validate_user_id, AuthError, Session};
pub use url::Url;

mod session_manager;
pub use session_manager::{AccountSession, AccountSessionError, SessionFuture, SessionPersistence};
