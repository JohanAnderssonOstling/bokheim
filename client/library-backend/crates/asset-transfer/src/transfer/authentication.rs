//! Library account renewal around a transfer operation, with at most one retry.
use crate::TransferError;
use library_services::account::{AccountSessionError, AuthenticatedRequestError, RefreshPolicy, authenticated_request};
use std::future::Future;

impl super::AssetExecutor {
    /// Capture credentials and the user before work so a retry cannot adopt
    /// an account that signed in while the operation was in flight.
    pub(super) async fn refreshed<T, F, Fut>(&self, mut operation: F) -> Result<T, TransferError>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, TransferError>>,
    {
        let credentials = self.credentials().await?;
        let Some(account) = self.authentication.account() else { return operation().await };
        authenticated_request(
            account,
            RefreshPolicy::OnRejection,
            |_| {
                let check = self.check_transfer_account(&credentials);
                let pending = operation();
                async move {
                    check.await?;
                    pending.await
                }
            },
            |error| matches!(error, TransferError::AuthenticationRequired),
        )
        .await
        .map_err(|error| match error {
            AuthenticatedRequestError::Request(error) => error,
            AuthenticatedRequestError::Session(AccountSessionError::Changed) | AuthenticatedRequestError::MissingSession => TransferError::AuthenticationRequired,
            AuthenticatedRequestError::Session(error) => TransferError::retryable(error),
        })
    }
}
