//! Library account renewal around a transfer operation, with at most one retry.
use super::{TransferError, TransferWorker};

#[derive(Clone, Copy)]
pub(super) enum RefreshScope {
    /// Planning/publication requires credentials before starting and validates the server binding.
    Planned,
    /// Queued execution may settle local work without credentials; revoked sessions pause it.
    Queued,
}

impl TransferWorker {
    pub(super) async fn with_refresh<T, F: std::future::Future<Output = Result<T, TransferError>>>(&self, scope: RefreshScope, mut operation: impl FnMut() -> F) -> Result<T, TransferError> {
        let planned = matches!(scope, RefreshScope::Planned);
        let credentials = self.credentials.credentials();
        if planned && credentials.is_none() {
            return Err(TransferError::AuthenticationRequired);
        }
        let user = if planned || self.account.is_some() { client_library::sync_state::get_sync_metadata(&*self.db_pool.get()?, "bound_user_id")? } else { None };
        let result = operation().await;
        if !matches!(result, Err(TransferError::AuthenticationRequired)) {
            return result;
        }
        let (Some(account), Some(credentials)) = (&self.account, credentials) else {
            return result;
        };
        match account.refresh_after_unauthorized(credentials.access_token()).await {
            Ok(Some(session)) if user.as_deref() == Some(session.user_id()) => {
                let conn = self.db_pool.get()?;
                if planned {
                    self.check_transfer_account(&conn, &user, &credentials)?;
                } else if client_library::sync_state::get_sync_metadata(&conn, "bound_user_id")? != user {
                    return Err(TransferError::AuthenticationRequired);
                }
                // Never retain a database borrow across transfer I/O.
                drop(conn);
                operation().await
            }
            Ok(_) => Err(TransferError::AuthenticationRequired),
            Err(_) if !planned && self.credentials.credentials().is_none() => Err(TransferError::AuthenticationRequired),
            Err(error) => Err(TransferError::retryable(error)),
        }
    }
}
