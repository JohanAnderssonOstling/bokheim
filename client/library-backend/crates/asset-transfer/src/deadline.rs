//! Deadlines release owned request resources on cancellation.
use crate::TransferError;
use client_platform_runtime::executor::timeout;
use std::{future::Future, time::Duration};

pub async fn transfer_deadline<T>(duration: Duration, request: impl Future<Output = T>) -> Result<T, TransferError> {
    timeout(duration, request)
        .await
        .map_err(|_| TransferError::Retryable(client_protocol::BackendError::classified(client_protocol::Failure::TimedOut, format!("asset request timed out after {} seconds", duration.as_secs()))))
}
