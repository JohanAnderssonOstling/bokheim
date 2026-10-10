 //! Deadlines release owned request resources on cancellation.
#[cfg(test)]
use crate::policy::SMALL_REQUEST_TIMEOUT;
use crate::TransferError;

pub async fn transfer_deadline<T>(duration: std::time::Duration, request: impl std::future::Future<Output = T>) -> Result<T, TransferError> {
    client_platform_runtime::executor::timeout(duration, request).await.map_err(|_| TransferError::retryable(format!("asset request timed out after {} seconds", duration.as_secs())))
}
