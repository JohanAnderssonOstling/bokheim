mod account;
mod auth;
mod session;

pub use account::{cloud_storage, set_cloud_storage, account_snapshot, current_account, delete_library, list_account_sessions, revoke_account_session, revoke_other_account_sessions, create_library, rename_library};
pub use account_contract::{AccountSnapshotResponse, LibraryStorageUsage, OtherSessionsRevocationResponse, SessionRevocationResponse, SessionSummary, StorageUsageResponse};
pub use auth::{login, logout, refresh_session, request_password_reset, request_public_registration, resend_verification, reset_password, verify_email};
pub use binary_http::{ServerUrl, ServerUrlError};
pub use session::{validate_user_id, AuthError, Session};

mod session_manager;
pub use session_manager::{AccountSession, AccountSessionError, SessionFuture, SessionPersistence};
