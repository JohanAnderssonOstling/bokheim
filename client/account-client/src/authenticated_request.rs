//! One authorization attempt and at most one renewal, pinned to its account.

use crate::{AccountSession, AccountSessionError, AuthError, Session, SessionFuture, SessionPersistence};
use std::future::Future;

/// Account capabilities can restrict which live session is authorized without
/// reimplementing request renewal or exposing their persistence owner.
pub trait SessionAuthority {
    fn current(&self) -> Result<Option<Session>, AccountSessionError>;
    fn refresh_if_needed(&self) -> SessionFuture<'_, Result<Option<Session>, AccountSessionError>>;
    fn refresh_after_unauthorized(&self, rejected_token: String) -> SessionFuture<'_, Result<Option<Session>, AccountSessionError>>;
}

#[derive(Clone, Copy, Debug)]
pub enum RefreshPolicy {
    BeforeRequest,
    OnRejection,
}

/// Adapters retain their own operation errors and choose how authorization
/// failures are presented at their boundary.
#[derive(Debug)]
pub enum AuthenticatedRequestError<E> {
    Request(E),
    Session(AccountSessionError),
    MissingSession,
}

pub async fn authenticated_request<A, T, E, F, Fut>(authority: &A, policy: RefreshPolicy, mut request: F, unauthorized: impl Fn(&E) -> bool) -> Result<T, AuthenticatedRequestError<E>>
where
    A: SessionAuthority + ?Sized,
    F: FnMut(Session) -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    use AuthenticatedRequestError::{MissingSession, Request, Session as SessionError};

    let session = match policy {
        RefreshPolicy::BeforeRequest => authority.refresh_if_needed().await,
        RefreshPolicy::OnRejection => authority.current(),
    }
    .map_err(SessionError)?
    .ok_or(MissingSession)?;
    let original = session.clone();
    match request(session).await {
        Err(error) if unauthorized(&error) => {
            let renewed = authority.refresh_after_unauthorized(original.token().to_owned()).await.map_err(SessionError)?.ok_or(MissingSession)?;
            // A newer token for the same user is valid. A new login must not
            // turn an old operation into a request against another account.
            if renewed.server_url() != original.server_url() || renewed.user_id() != original.user_id() {
                return Err(SessionError(AccountSessionError::Changed));
            }
            request(renewed).await.map_err(Request)
        }
        result => result.map_err(Request),
    }
}

impl<P: SessionPersistence> SessionAuthority for AccountSession<P> {
    fn current(&self) -> Result<Option<Session>, AccountSessionError> {
        AccountSession::current(self)
    }

    fn refresh_if_needed(&self) -> SessionFuture<'_, Result<Option<Session>, AccountSessionError>> {
        Box::pin(AccountSession::refresh_if_needed(self))
    }

    fn refresh_after_unauthorized(&self, rejected_token: String) -> SessionFuture<'_, Result<Option<Session>, AccountSessionError>> {
        Box::pin(async move { AccountSession::refresh_after_unauthorized(self, &rejected_token).await })
    }
}

pub async fn account_request<A, T, Fut>(authority: &A, request: impl FnMut(Session) -> Fut) -> Result<T, AccountSessionError>
where
    A: SessionAuthority + ?Sized,
    Fut: Future<Output = Result<T, AuthError>>,
{
    authenticated_request(authority, RefreshPolicy::BeforeRequest, request, AuthError::is_unauthorized).await.map_err(|error| match error {
        AuthenticatedRequestError::Request(error) => AccountSessionError::Authentication(error),
        AuthenticatedRequestError::Session(error) => error,
        AuthenticatedRequestError::MissingSession => AccountSessionError::Authentication(AuthError::SignInRequired("not signed in")),
    })
}
