//! Authentication session persistence and account client types.

pub use sync_client::{AuthError, Session};

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use uuid::Uuid;

fn write_private_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "session path has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".session-{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn load_session_from(path: &Path) -> std::io::Result<Option<Session>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    serde_json::from_slice(&bytes).map(Some).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

pub fn save_session_to(session: &Session, path: &Path) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(session).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    write_private_atomic(path, &bytes)
}

pub fn clear_session_from(path: &Path) -> std::io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {
            #[cfg(unix)]
            fs::File::open(path.parent().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "session path has no parent"))?)?.sync_all()?;
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn desktop_session_path() -> std::io::Result<PathBuf> {
    dirs::home_dir().map(|home| home.join(crate::APP_HIDDEN_SESSION_PATH)).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "desktop home directory is unavailable"))
}

pub fn load_session() -> std::io::Result<Option<Session>> {
    load_session_from(&desktop_session_path()?)
}
pub fn save_session(session: &Session) -> std::io::Result<()> {
    save_session_to(session, &desktop_session_path()?)
}
pub fn clear_session() -> std::io::Result<()> {
    clear_session_from(&desktop_session_path()?)
}

pub async fn login(email: &str, password: &str) -> Result<Session, AuthError> {
    sync_client::login(&crate::api_server_url(), email, password).await
}

pub async fn request_public_registration(email: &str, password: &str) -> Result<(), AuthError> {
    sync_client::request_public_registration(&crate::api_server_url(), email, password).await
}

pub async fn resend_verification(email: &str) -> Result<(), AuthError> {
    sync_client::resend_verification(&crate::api_server_url(), email).await
}

pub async fn verify_email(email: &str, pin: &str) -> Result<Session, AuthError> {
    sync_client::verify_email(&crate::api_server_url(), email, pin).await
}

pub async fn request_password_reset(email: &str) -> Result<(), AuthError> {
    sync_client::request_password_reset(&crate::api_server_url(), email).await
}

pub async fn reset_password(token: &str, new_password: &str) -> Result<(), AuthError> {
    sync_client::reset_password(&crate::api_server_url(), token, new_password).await
}

fn require_api_session(session: &Session) -> Result<(), AuthError> {
    if session.server_url() == &crate::api_server_url() {
        Ok(())
    } else {
        Err(AuthError::Binding(format!("server communication must use {}", crate::API_SERVER_ORIGIN)))
    }
}

pub async fn logout(session: &Session) -> Result<(), AuthError> {
    require_api_session(session)?;
    sync_client::logout(session).await
}

pub async fn refresh_session(session: &Session) -> Result<Session, AuthError> {
    require_api_session(session)?;
    sync_client::refresh_session(session).await
}

pub async fn list_libraries(session: &Session) -> Result<Vec<sync_common::api::libraries::LibrarySummary>, AuthError> {
    require_api_session(session)?;
    sync_client::list_libraries(session).await
}

pub async fn list_deleted_libraries(session: &Session) -> Result<sync_common::api::libraries::DeletedLibrariesResponse, AuthError> {
    require_api_session(session)?;
    sync_client::list_deleted_libraries(session).await
}

pub async fn list_account_sessions(session: &Session) -> Result<Vec<sync_client::SessionSummary>, AuthError> {
    require_api_session(session)?;
    sync_client::list_account_sessions(session).await
}

pub async fn account_storage_usage(session: &Session) -> Result<sync_client::StorageUsageResponse, AuthError> {
    require_api_session(session)?;
    sync_client::account_storage_usage(session).await
}

pub async fn account_library_storage_usage(session: &Session) -> Result<sync_client::LibraryStorageUsageResponse, AuthError> {
    require_api_session(session)?;
    sync_client::account_library_storage_usage(session).await
}

pub async fn revoke_account_session(session: &Session, session_id: &str) -> Result<sync_client::SessionRevocationResponse, AuthError> {
    require_api_session(session)?;
    sync_client::revoke_account_session(session, session_id).await
}

pub async fn revoke_other_account_sessions(session: &Session) -> Result<sync_client::OtherSessionsRevocationResponse, AuthError> {
    require_api_session(session)?;
    sync_client::revoke_other_account_sessions(session).await
}

pub async fn upsert_library(session: &Session, library_id: sync_common::LibraryId, library_name: String) -> Result<sync_common::api::libraries::LibrarySummary, AuthError> {
    require_api_session(session)?;
    sync_client::upsert_library(session, library_id, library_name).await
}

pub async fn delete_library(session: &Session, library_id: sync_common::LibraryId) -> Result<sync_common::api::libraries::DeleteLibraryResponse, AuthError> {
    require_api_session(session)?;
    sync_client::delete_library(session, &library_id).await
}

fn blocking_runtime() -> Result<tokio::runtime::Runtime, AuthError> {
    tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(AuthError::Io)
}

pub fn login_blocking(email: &str, password: &str) -> Result<Session, AuthError> {
    blocking_runtime()?.block_on(login(email, password))
}
pub fn request_public_registration_blocking(email: &str, password: &str) -> Result<(), AuthError> {
    blocking_runtime()?.block_on(request_public_registration(email, password))
}
pub fn resend_verification_blocking(email: &str) -> Result<(), AuthError> {
    blocking_runtime()?.block_on(resend_verification(email))
}
pub fn verify_email_blocking(email: &str, pin: &str) -> Result<Session, AuthError> {
    blocking_runtime()?.block_on(verify_email(email, pin))
}
pub fn request_password_reset_blocking(email: &str) -> Result<(), AuthError> {
    blocking_runtime()?.block_on(request_password_reset(email))
}
pub fn reset_password_blocking(token: &str, new_password: &str) -> Result<(), AuthError> {
    blocking_runtime()?.block_on(reset_password(token, new_password))
}
pub fn logout_blocking(session: &Session) -> Result<(), AuthError> {
    blocking_runtime()?.block_on(logout(session))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_session_round_trips_through_native_adapter() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("session.json");
        let session = Session::try_new(crate::api_server_url(), "user-1".into(), "reader".into(), "a".repeat(43)).unwrap();
        save_session_to(&session, &path).unwrap();
        let restored = load_session_from(&path).unwrap().unwrap();
        assert_eq!(restored.user_id(), session.user_id());
        clear_session_from(&path).unwrap();
        assert!(load_session_from(&path).unwrap().is_none());
    }

    #[test]
    fn production_requests_reject_non_api_sessions() {
        let session = Session::try_new(crate::ServerUrl::parse("https://example.com").unwrap(), "user-1".into(), "reader".into(), "a".repeat(43)).unwrap();
        assert!(require_api_session(&session).is_err());
    }
}
