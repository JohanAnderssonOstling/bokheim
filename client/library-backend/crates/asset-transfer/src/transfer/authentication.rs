//! Library account renewal around a transfer operation, with at most one retry.
use crate::TransferError;

/// The pre-flight half of a refreshable operation: credentials and user are
/// captured before any work so a refresh for another session cannot silently
/// adopt the operation.
pub(super) struct RefreshGuard {
    credentials: sync_transport::SyncCredentials,
    user: Option<String>,
}

impl super::AssetExecutor {
    pub(super) fn refresh_guard(&self) -> Result<RefreshGuard, TransferError> {
        Ok(RefreshGuard { credentials: self.credentials()?, user: self.live_user_id() })
    }

    /// Renew the same account after an unauthorized request.
    pub(super) async fn refresh_after_auth(&self, guard: RefreshGuard) -> Result<(), TransferError> {
        let Some(account) = &self.account else {
            return Err(TransferError::AuthenticationRequired);
        };
        match account.refresh_after_unauthorized(guard.credentials.access_token()).await {
            Ok(Some(session)) if guard.user.as_deref() == Some(session.user_id()) => {
                self.check_transfer_account(&guard.credentials)?;
                Ok(())
            }
            Ok(_) => Err(TransferError::AuthenticationRequired),
            Err(error) => Err(TransferError::retryable(error)),
        }
    }
}
