//! Account capability injected into library-owned remote workers.

use std::{fmt, ops::Deref, sync::Arc};

use account_client::{AccountSession, Session, SessionFuture, SessionPersistence};
use sync_common::LibraryId;
use sync_common::storage::LibraryStorage;
use sync_transport::{CredentialsSource, SyncCredentials};
use tokio::sync::watch::Receiver;

pub use account_client::AccountSessionError;

pub type SharedSyncCredentials = Arc<dyn CredentialsSource>;

/// One shared session supplies account operations and sync credentials.
/// Async operations own a handle so their futures can outlive the caller.
trait AccountService: CredentialsSource {
    fn current(&self) -> Result<Option<Session>, AccountSessionError>;
    fn require(&self) -> Result<Session, AccountSessionError>;
    fn subscribe(&self) -> Receiver<u64>;
    fn refresh_if_needed(self: Arc<Self>) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>>;
    fn refresh_after_unauthorized(self: Arc<Self>, token: String) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>>;
    fn create_library(self: Arc<Self>, library_id: LibraryId, name: String, storage: LibraryStorage) -> SessionFuture<'static, Result<(), AccountSessionError>>;
}

#[derive(Clone)]
pub struct LibraryAccount {
    service: Arc<dyn AccountService>,
}

impl fmt::Debug for LibraryAccount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("LibraryAccount").finish_non_exhaustive()
    }
}

impl LibraryAccount {
    pub fn current(&self) -> Result<Option<Session>, AccountSessionError> {
        self.service.current()
    }
    pub fn require(&self) -> Result<Session, AccountSessionError> {
        self.service.require()
    }
    pub fn subscribe(&self) -> Receiver<u64> {
        self.service.subscribe()
    }
    pub fn refresh_if_needed(&self) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>> {
        self.service.clone().refresh_if_needed()
    }
    pub fn refresh_after_unauthorized(&self, token: impl Into<String>) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>> {
        self.service.clone().refresh_after_unauthorized(token.into())
    }
    pub fn create_library(&self, library_id: LibraryId, name: String, storage: LibraryStorage) -> SessionFuture<'static, Result<(), AccountSessionError>> {
        self.service.clone().create_library(library_id, name, storage)
    }
    pub fn credentials(&self) -> SharedSyncCredentials {
        self.service.clone()
    }
}

#[derive(Clone, Debug)]
pub struct LibraryAccountSession<P: SessionPersistence>(AccountSession<P>);

impl<P: SessionPersistence> LibraryAccountSession<P> {
    pub fn load(persistence: P, server: account_client::Url) -> Result<Self, AccountSessionError> {
        AccountSession::load(persistence, server).map(Self)
    }
}

impl<P> LibraryAccountSession<P>
where
    P: SessionPersistence + Send + Sync + 'static,
{
    pub fn library_account(&self) -> LibraryAccount {
        LibraryAccount { service: Arc::new(self.clone()) }
    }
}

impl<P> AccountService for LibraryAccountSession<P>
where
    P: SessionPersistence + Send + Sync + 'static,
{
    fn current(&self) -> Result<Option<Session>, AccountSessionError> {
        self.0.current()
    }
    fn require(&self) -> Result<Session, AccountSessionError> {
        self.0.require()
    }
    fn subscribe(&self) -> Receiver<u64> {
        self.0.subscribe()
    }
    fn refresh_if_needed(self: Arc<Self>) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>> {
        Box::pin(async move { self.0.refresh_if_needed().await })
    }
    fn refresh_after_unauthorized(self: Arc<Self>, token: String) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>> {
        Box::pin(async move { self.0.refresh_after_unauthorized(&token).await })
    }
    fn create_library(self: Arc<Self>, library_id: LibraryId, name: String, storage: LibraryStorage) -> SessionFuture<'static, Result<(), AccountSessionError>> {
        Box::pin(async move { self.0.request(|session| Box::pin(account_client::create_library_with_storage(session, library_id, name.clone(), storage.clone()))).await.map(|_| ()) })
    }
}

impl<P: SessionPersistence> Deref for LibraryAccountSession<P> {
    type Target = AccountSession<P>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<P> CredentialsSource for LibraryAccountSession<P>
where
    P: SessionPersistence + Send + Sync,
{
    fn credentials(&self) -> Option<SyncCredentials> {
        self.credentials_session().map(|session| SyncCredentials::new(session.server_url().clone(), session.token()))
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[derive(Clone, Debug)]
    struct EmptyPersistence;

    impl SessionPersistence for EmptyPersistence {
        fn load_account_session(&self) -> std::io::Result<Option<Session>> {
            Ok(None)
        }
        fn save_account_session(&self, _: &Session) -> std::io::Result<()> {
            Ok(())
        }
        fn clear_account_session(&self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn account() -> LibraryAccountSession<EmptyPersistence> {
        let server = "https://example.com".parse().unwrap();
        let account = LibraryAccountSession::load(EmptyPersistence, server).unwrap();
        let session = Session::try_new("https://example.com".parse().unwrap(), "user".into(), "reader@example.com".into(), "a".repeat(43)).unwrap();
        account.set(session).unwrap();
        account
    }

    #[tokio::test]
    async fn unauthorized_refresh_updates_clones_credentials_and_subscribers() {
        let session = account();
        let account = session.library_account();
        let clone = account.clone();
        let credentials = clone.credentials();
        let mut changes = clone.subscribe();
        let token = account.require().unwrap().token().to_owned();
        assert_eq!(credentials.credentials().unwrap().access_token(), token);

        // A rejected legacy token has no refresh token and signs the account out.
        assert!(account.refresh_after_unauthorized(token).await.unwrap().is_none());
        assert!(clone.current().unwrap().is_none());
        assert!(credentials.credentials().is_none());
        assert!(session.current().unwrap().is_none());
        assert!(changes.has_changed().unwrap());
        assert_eq!(*changes.borrow_and_update(), 2);
    }

    #[tokio::test]
    async fn pending_refreshes_and_credentials_outlive_account_handles() {
        let (refresh, stale_rejection, credentials) = {
            let session = account();
            let account = session.library_account();
            (account.refresh_if_needed(), account.refresh_after_unauthorized("b".repeat(43)), account.credentials())
        };

        let token = "a".repeat(43);
        assert_eq!(refresh.await.unwrap().unwrap().token(), token);
        assert_eq!(stale_rejection.await.unwrap().unwrap().token(), token);
        assert_eq!(credentials.credentials().unwrap().access_token(), token);
    }
}
