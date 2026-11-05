//! Account capability injected into library-owned remote workers.

use std::{fmt, future::Future, ops::Deref, sync::Arc};

use account_client::{AccountSession, AccountStatus, AuthError, Session, SessionAuthority, SessionFuture, SessionPersistence};
use client_platform_runtime::storage_queue;
use client_protocol::{BackendError, Failure};
use sync_transport::{CredentialsFuture, CredentialsSource, SyncCredentials};
use tokio::sync::watch::Receiver;

pub use account_client::{AccountSessionError, AuthenticatedRequestError, RefreshPolicy, authenticated_request};

/// An account request's error, with what the account client knows about it.
pub fn auth_error(error: AuthError) -> BackendError {
    match error {
        AuthError::SignInRequired(reason) => BackendError::classified(Failure::SignInRequired, reason),
        AuthError::PasswordTooLong => BackendError::classified(Failure::PasswordTooLong, "password exceeds the maximum length"),
        error @ AuthError::Http(_) if error.is_upgrade_required() => BackendError::classified(Failure::ClientUpgradeRequired, error.to_string()),
        AuthError::Wire(sync_common::transport::WireError::UnsupportedVersion(version)) => BackendError::classified(Failure::ClientUpgradeRequired, format!("unsupported account response protocol version {version}")),
        error => BackendError::operation(error),
    }
}

/// An account session's error, with what the account client knows about it.
pub fn account_error(error: AccountSessionError) -> BackendError {
    match error {
        AccountSessionError::Authentication(error) => auth_error(error),
        error @ (AccountSessionError::Changed | AccountSessionError::DeletionIdentityMismatch) => BackendError::classified(Failure::AccountChanged, error.to_string()),
        error => BackendError::operation(error),
    }
}

pub type SharedSyncCredentials = Arc<dyn CredentialsSource>;

/// A managed account supplies both renewal and its live credentials. A caller
/// that supplies credentials alone explicitly opts out of account renewal.
#[derive(Clone)]
pub enum LibraryAuthentication {
    Account(LibraryAccount),
    Credentials(SharedSyncCredentials),
}

impl LibraryAuthentication {
    pub async fn credentials(&self) -> Option<SyncCredentials> {
        match self {
            Self::Account(account) => CredentialsSource::credentials(account).await,
            Self::Credentials(credentials) => credentials.credentials().await,
        }
    }

    pub fn account(&self) -> Option<&LibraryAccount> {
        match self {
            Self::Account(account) => Some(account),
            Self::Credentials(_) => None,
        }
    }
}

/// One shared session supplies account operations and sync credentials.
/// Async operations own a handle so their futures can outlive the caller.
trait AccountService: CredentialsSource {
    fn current(&self) -> Result<Option<Session>, AccountSessionError>;
    fn subscribe(&self) -> Receiver<u64>;
    fn subscribe_status(&self) -> Receiver<AccountStatus>;
    fn refresh_if_needed(self: Arc<Self>) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>>;
    fn refresh_after_unauthorized(self: Arc<Self>, token: String) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>>;
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

    /// Synchronous access for management transactions already on the storage owner.
    pub fn current(&self) -> Result<Option<Session>, AccountSessionError> {
        self.authorized_session(self.service.current()?)
    }
    pub fn require(&self) -> Result<Session, AccountSessionError> {
        self.current()?.ok_or(AccountSessionError::Authentication(AuthError::SignInRequired("sign in to the account that owns this library to access its server books")))
    }
    /// Authorization includes persisted library ownership and must run with storage.
    pub async fn session(&self) -> Result<Option<Session>, AccountSessionError> {
        let account = self.clone();
        storage_queue::run(move || account.current()).await.map_err(|error| AccountSessionError::Persistence(std::io::Error::other(error)))?
    }
    pub async fn require_session(&self) -> Result<Session, AccountSessionError> {
        self.session().await?.ok_or(AccountSessionError::Authentication(AuthError::SignInRequired("sign in to the account that owns this library to access its server books")))
    }
    pub fn subscribe(&self) -> Receiver<u64> {
        self.service.subscribe()
    }
    pub fn subscribe_status(&self) -> Receiver<AccountStatus> {
        self.service.subscribe_status()
    }

    /// Cancel remote work when its account logs out or changes. Same-account
    /// token renewal does not interrupt the operation. Subscribe before checking
    /// access so even a rapid logout/login invalidates the previous operation.
    pub async fn while_authorized<T>(&self, operation: impl Future<Output = T>) -> Result<T, AccountSessionError> {
        let mut status = self.subscribe_status();
        self.require_session().await?;
        tokio::select! {
            biased;
            _ = status.changed() => Err(AccountSessionError::Changed),
            result = operation => {
                self.require_session().await?;
                if status.has_changed().unwrap_or(true) {
                    return Err(AccountSessionError::Changed);
                }
                Ok(result)
            },
        }
    }
    pub fn refresh_if_needed(&self) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>> {
        let account = self.clone();
        Box::pin(async move {
            if account.session().await?.is_none() {
                return Ok(None);
            }
            let session = account.service.clone().refresh_if_needed().await?;
            storage_queue::run(move || account.authorized_session(session)).await.map_err(|error| AccountSessionError::Persistence(std::io::Error::other(error)))?
        })
    }
    pub fn refresh_after_unauthorized(&self, token: impl Into<String>) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>> {
        let account = self.clone();
        let token = token.into();
        Box::pin(async move {
            if account.session().await?.is_none() {
                return Ok(None);
            }
            let session = account.service.clone().refresh_after_unauthorized(token).await?;
            storage_queue::run(move || account.authorized_session(session)).await.map_err(|error| AccountSessionError::Persistence(std::io::Error::other(error)))?
        })
    }
    pub async fn request<T, Fut>(&self, request: impl FnMut(Session) -> Fut) -> Result<T, AccountSessionError>
    where
        Fut: std::future::Future<Output = Result<T, AuthError>>,
    {
        self.while_authorized(account_client::account_request(self, request)).await?
    }
    pub fn credentials(&self) -> SharedSyncCredentials {
        Arc::new(self.clone())
    }
}

impl CredentialsSource for LibraryAccount {
    fn credentials(&self) -> CredentialsFuture<'_> {
        Box::pin(async move { self.session().await.ok().flatten().map(|session| SyncCredentials::new(session.server_url().clone(), session.token())) })
    }
}

impl SessionAuthority for LibraryAccount {
    fn current(&self) -> SessionFuture<'_, Result<Option<Session>, AccountSessionError>> {
        Box::pin(self.session())
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
    fn subscribe_status(&self) -> Receiver<AccountStatus> {
        self.0.subscribe_status()
    }
    fn refresh_if_needed(self: Arc<Self>) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>> {
        Box::pin(async move { self.0.refresh_if_needed().await })
    }
    fn refresh_after_unauthorized(self: Arc<Self>, token: String) -> SessionFuture<'static, Result<Option<Session>, AccountSessionError>> {
        Box::pin(async move { self.0.refresh_after_unauthorized(&token).await })
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
    fn credentials(&self) -> CredentialsFuture<'_> {
        Box::pin(async move { self.credentials_session().map(|session| SyncCredentials::new(session.server_url().clone(), session.token())) })
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
        let session = Session::try_new("https://example.com".parse().unwrap(), "user".into(), "reader@example.com".into(), "a".repeat(43), i64::MAX - 1, "r".repeat(43), i64::MAX).unwrap();
        account.set(session).unwrap();
        account
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
        assert_eq!(credentials.credentials().await.unwrap().access_token(), token);
    }

    #[tokio::test]
    async fn library_credentials_do_not_follow_another_account_and_resume_for_the_owner() {
        let session = account();
        let library = session.library_account().with_authorization(|session| Ok(session.user_id() == "user"));
        let credentials = library.credentials();
        let mut changes = library.subscribe();
        assert!(credentials.credentials().await.is_some());

        session.take().unwrap();
        assert!(library.current().unwrap().is_none());
        assert!(credentials.credentials().await.is_none());
        let other = Session::try_new("https://example.com".parse().unwrap(), "other".into(), "other@example.com".into(), "b".repeat(43), i64::MAX - 1, "r".repeat(43), i64::MAX).unwrap();
        session.set(other).unwrap();
        assert!(changes.has_changed().unwrap());
        changes.borrow_and_update();
        assert!(library.current().unwrap().is_none());
        assert!(credentials.credentials().await.is_none());
        assert!(library.require().is_err());
        assert!(library.refresh_if_needed().await.unwrap().is_none());
        assert!(library.refresh_after_unauthorized("a".repeat(43)).await.unwrap().is_none());
        assert!(library.request(|_| async { Ok(()) }).await.is_err());
        assert_eq!(session.require().unwrap().user_id(), "other", "a library must not sign out the other account");

        let owner = Session::try_new("https://example.com".parse().unwrap(), "user".into(), "reader@example.com".into(), "c".repeat(43), i64::MAX - 1, "r".repeat(43), i64::MAX).unwrap();
        session.set(owner).unwrap();
        assert!(changes.has_changed().unwrap());
        assert_eq!(library.require().unwrap().user_id(), "user");
        assert_eq!(credentials.credentials().await.unwrap().access_token(), "c".repeat(43));
    }
}
