//! Deadlines release owned request resources on cancellation.
use crate::TransferError;
use client_platform_runtime::{executor::timeout, transfers};
use library_model::TransferOrigin;
use std::{future::Future, time::Duration};

pub async fn transfer_deadline<T>(duration: Duration, request: impl Future<Output = T>) -> Result<T, TransferError> {
    download_deadline(TransferOrigin::Background, duration, request).await
}

pub async fn download_deadline<T>(origin: TransferOrigin, duration: Duration, request: impl Future<Output = T>) -> Result<T, TransferError> {
    let request = timeout(duration, request);
    let result = match origin {
        TransferOrigin::UserInitiated => request.await,
        TransferOrigin::Background => transfers::try_run(request).await.map_err(TransferError::retryable)?,
    };
    result.map_err(|_| TransferError::Retryable(client_runtime::BackendError::classified(client_runtime::Failure::TimedOut, format!("asset request timed out after {} seconds", duration.as_secs()))))
}
