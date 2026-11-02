//! Account capability injected into library-owned remote workers.

use std::{fmt, ops::Deref, sync::Arc};

use account_client::{AccountSession, Session, SessionAuthority, SessionFuture, SessionPersistence};
use sync_common::LibraryId;
use sync_common::storage::{LibraryKind, LibraryStorage};
use sync_transport::{CredentialsSource, SyncCredentials};
use tokio::sync::watch::Receiver;

pub use account_client::{AccountSessionError, AuthenticatedRequestError, RefreshPolicy, authenticated_request};

pub type SharedSyncCredentials = Arc<dyn CredentialsSource>;

/// A managed account supplies both renewal and its live credentials. A caller
/// that supplies credentials alone explicitly opts out of account renewal.
#[derive(Clone)]
pub enum LibraryAuthentication {
    Account(LibraryAccount),
    Credentials(SharedSyncCredentials),
}

impl LibraryAuthentication {
    pub fn credentials(&self) -> SharedSyncCredentials {
        match self {
            Self::Account(account) => account.credentials(),
            Self::Credentials(credentials) => credentials.clone(),
        }
    }

    pub fn account(&self) -> Option<LibraryAccount> {
        match self {
            Self::Account(account) => Some(account.clone()),
            Self::Credentials(_) => None,
        }
    }
}

/// One shared session supplies account operations and sync credentials.
/// Async operations own a handle so their futures can outlive the caller.
trait AccountService: CredentialsSource {
    fn current(&self) -> Result<Option<Session>, AccountSessionError>;
    fn subscribe(&self) -> Receiver<u64>;
    fn refresh_if_needed(self: Arc<Self>) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>>;
    fn refresh_after_unauthorized(self: Arc<Self>, token: String) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>>;
    fn create_library(self: Arc<Self>, expected: Session, library_id: LibraryId, name: String, storage: LibraryStorage, kind: LibraryKind) -> SessionFuture<'static, Result<(), AccountSessionError>>;
}

type Authorization = Arc<dyn Fn(&Session) -> std::io::Result<bool> + Send + Sync>;

#[derive(Clone)]
pub struct LibraryAccount {
    service: Arc<dyn AccountService>,
    authorization: Option<Authorization>,
}

impl fmt::Debug for LibraryAccount {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("LibraryAccount").finish_non_exhaustive()
    }
}

impl LibraryAccount {
    /// Restrict remote operations to the library's persisted account while
    /// retaining the same live session and account-change subscription.
    pub fn with_authorization(mut self, authorize: impl Fn(&Session) -> std::io::Result<bool> + Send + Sync + 'static) -> Self {
        self.authorization = Some(Arc::new(authorize));
        self
    }

    fn authorized_session(&self, session: Option<Session>) -> Result<Option<Session>, AccountSessionError> {
        let Some(session) = session else { return Ok(None) };
        let authorized = self.authorization.as_ref().map_or(Ok(true), |authorize| authorize(&session).map_err(AccountSessionError::Persistence))?;
        Ok(authorized.then_some(session))
    }

    pub fn current(&self) -> Result<Option<Session>, AccountSessionError> {
        self.authorized_session(self.service.current()?)
    }
    pub fn require(&self) -> Result<Session, AccountSessionError> {
        self.current()?.ok_or(AccountSessionError::Authentication(account_client::AuthError::SignInRequired("sign in to the account that owns this library to access its server books")))
    }
    pub fn subscribe(&self) -> Receiver<u64> {
        self.service.subscribe()
    }
    pub fn refresh_if_needed(&self) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>> {
        let account = self.clone();
        Box::pin(async move {
            if account.current()?.is_none() {
                return Ok(None);
            }
            account.authorized_session(account.service.clone().refresh_if_needed().await?)
        })
    }
    pub fn refresh_after_unauthorized(&self, token: impl Into<String>) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>> {
        let account = self.clone();
        let token = token.into();
        Box::pin(async move {
            if account.current()?.is_none() {
                return Ok(None);
            }
            account.authorized_session(account.service.clone().refresh_after_unauthorized(token).await?)
        })
    }
    pub fn create_library(&self, library_id: LibraryId, name: String, storage: LibraryStorage, kind: LibraryKind) -> SessionFuture<'static, Result<(), AccountSessionError>> {
        let account = self.clone();
        Box::pin(async move {
            let session = account.require()?;
            account.service.create_library(session, library_id, name, storage, kind).await
        })
    }
    pub async fn request<T, Fut>(&self, request: impl FnMut(Session) -> Fut) -> Result<T, AccountSessionError>
    where
        Fut: std::future::Future<Output = Result<T, account_client::AuthError>>,
    {
        account_client::account_request(self, request).await
    }
    pub fn credentials(&self) -> SharedSyncCredentials {
        Arc::new(self.clone())
    }
}

impl CredentialsSource for LibraryAccount {
    fn credentials(&self) -> Option<SyncCredentials> {
        self.current().ok().flatten().map(|session| SyncCredentials::new(session.server_url().clone(), session.token()))
    }
}

impl SessionAuthority for LibraryAccount {
    fn current(&self) -> Result<Option<Session>, AccountSessionError> {
        LibraryAccount::current(self)
    }

    fn refresh_if_needed(&self) -> SessionFuture<'_, Result<Option<Session>, AccountSessionError>> {
        LibraryAccount::refresh_if_needed(self)
    }

    fn refresh_after_unauthorized(&self, token: String) -> SessionFuture<'_, Result<Option<Session>, AccountSessionError>> {
        LibraryAccount::refresh_after_unauthorized(self, token)
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
        LibraryAccount { service: Arc::new(self.clone()), authorization: None }
    }
}

impl<P> AccountService for LibraryAccountSession<P>
where
    P: SessionPersistence + Send + Sync + 'static,
{
    fn current(&self) -> Result<Option<Session>, AccountSessionError> {
        self.0.current()
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
    fn create_library(self: Arc<Self>, expected: Session, library_id: LibraryId, name: String, storage: LibraryStorage, kind: LibraryKind) -> SessionFuture<'static, Result<(), AccountSessionError>> {
        Box::pin(async move {
            self.0
                .request(|session| {
                    let matches = session.server_url() == expected.server_url() && session.user_id() == expected.user_id();
                    let name = name.clone();
                    let storage = storage.clone();
                    let kind = kind.clone();
                    Box::pin(async move {
                        if !matches {
                            return Err(account_client::AuthError::SignInRequired("library belongs to another account"));
                        }
                        account_client::create_library_with_kind(&session, library_id, name, storage, kind).await
                    })
                })
                .await
                .map(|_| ())
        })
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

    #[tokio::test]
    async fn library_credentials_do_not_follow_another_account_and_resume_for_the_owner() {
        let session = account();
        let library = session.library_account().with_authorization(|session| Ok(session.user_id() == "user"));
        let credentials = library.credentials();
        let mut changes = library.subscribe();
        assert!(credentials.credentials().is_some());

        session.take().unwrap();
        assert!(library.current().unwrap().is_none());
        assert!(credentials.credentials().is_none());
        let other = Session::try_new("https://example.com".parse().unwrap(), "other".into(), "other@example.com".into(), "b".repeat(43)).unwrap();
        session.set(other).unwrap();
        assert!(changes.has_changed().unwrap());
        changes.borrow_and_update();
        assert!(library.current().unwrap().is_none());
        assert!(credentials.credentials().is_none());
        assert!(library.require().is_err());
        assert!(library.refresh_if_needed().await.unwrap().is_none());
        assert!(library.refresh_after_unauthorized("a".repeat(43)).await.unwrap().is_none());
        assert!(library.create_library(LibraryId::new_v4(), "Books".into(), LibraryStorage::Bokheim, LibraryKind::Standard).await.is_err());
        assert_eq!(session.require().unwrap().user_id(), "other", "a library must not sign out the other account");

        let owner = Session::try_new("https://example.com".parse().unwrap(), "user".into(), "reader@example.com".into(), "c".repeat(43)).unwrap();
        session.set(owner).unwrap();
        assert!(changes.has_changed().unwrap());
        assert_eq!(library.require().unwrap().user_id(), "user");
        assert_eq!(credentials.credentials().unwrap().access_token(), "c".repeat(43));
    }
}
