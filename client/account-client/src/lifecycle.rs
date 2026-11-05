//! Authentication prepares a change; the host coordinates access before committing it.
use crate::{AccountSession, AccountSessionError, AccountStatus, Session, SessionPersistence};

#[derive(Debug)]
pub enum SessionChange {
    SignIn(Session),
    SignOut,
    AccountDeleted { user_id: String },
}

/// Prepared authentication work has not changed local credentials yet.
/// The host can retire resources owned by the previous account before commit.
#[derive(Debug)]
#[must_use = "session changes must be committed to update local credentials"]
pub struct PendingSessionChange<P: SessionPersistence> {
    account: AccountSession<P>,
    change: SessionChange,
}

impl<P: SessionPersistence> PendingSessionChange<P> {
    pub fn change(&self) -> &SessionChange {
        &self.change
    }

    /// Persist and publish the change. Signing out returns the captured session
    /// so the host can revoke it without blocking a subsequent login.
    pub fn commit(self) -> Result<Option<Session>, AccountSessionError> {
        match self.change {
            SessionChange::SignIn(session) => self.account.set(session)?,
            SessionChange::SignOut => return self.account.take(),
            SessionChange::AccountDeleted { user_id } => self.account.clear_user(&user_id)?,
        }
        Ok(None)
    }
}

impl<P: SessionPersistence> AccountSession<P> {
    pub fn status(&self) -> Result<AccountStatus, AccountSessionError> {
        let session = match self.current()? {
            Some(session) => Some(session),
            None => self.restore_if_empty()?,
        };
        Ok(AccountStatus::from_session(session.as_ref()))
    }

    pub async fn prepare_login(&self, email: &str, password: &str) -> Result<PendingSessionChange<P>, AccountSessionError> {
        let session = crate::login(self.server_url(), email, password).await.map_err(AccountSessionError::Authentication)?;
        Ok(self.prepare_adoption(session))
    }

    pub async fn prepare_verification(&self, email: &str, pin: &str, password: &str) -> Result<PendingSessionChange<P>, AccountSessionError> {
        let session = crate::verify_email(self.server_url(), email, pin, password).await.map_err(AccountSessionError::Authentication)?;
        Ok(self.prepare_adoption(session))
    }

    pub fn prepare_adoption(&self, session: Session) -> PendingSessionChange<P> {
        PendingSessionChange { account: self.clone(), change: SessionChange::SignIn(session) }
    }

    pub fn prepare_logout(&self) -> PendingSessionChange<P> {
        PendingSessionChange { account: self.clone(), change: SessionChange::SignOut }
    }

    pub async fn prepare_deletion(&self, email: &str, password: &str) -> Result<PendingSessionChange<P>, AccountSessionError> {
        let session = self.session().await?.ok_or(AccountSessionError::Authentication(crate::AuthError::SignInRequired("not signed in")))?;
        if session.email() != email {
            return Err(AccountSessionError::DeletionIdentityMismatch);
        }
        crate::delete_account(&session, password).await.map_err(AccountSessionError::Authentication)?;
        Ok(PendingSessionChange { account: self.clone(), change: SessionChange::AccountDeleted { user_id: session.user_id().to_owned() } })
    }
}
