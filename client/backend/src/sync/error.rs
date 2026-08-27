use super::transfer::TransferError;

pub(crate) type SyncResult<T> = Result<T, SyncError>;

#[derive(Debug)]
pub(crate) enum SyncError {
    AuthenticationRequired,
    /// Used internally by state synchronization to trigger cursor recovery.
    CursorRejected,
    Failed(String),
}

impl SyncError {
    pub(super) fn failed(error: impl std::fmt::Display) -> Self {
        Self::Failed(error.to_string())
    }
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthenticationRequired => formatter.write_str("synchronization authentication is required"),
            Self::CursorRejected => formatter.write_str("the synchronization cursor was rejected after recovery"),
            Self::Failed(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for SyncError {}

impl From<Box<dyn std::error::Error>> for SyncError {
    fn from(error: Box<dyn std::error::Error>) -> Self {
        Self::failed(error)
    }
}

impl From<sync_client::SyncRequestError> for SyncError {
    fn from(error: sync_client::SyncRequestError) -> Self {
        match error {
            sync_client::SyncRequestError::AuthenticationRequired => Self::AuthenticationRequired,
            sync_client::SyncRequestError::CursorRejected => Self::CursorRejected,
            error @ sync_client::SyncRequestError::Other(_) => Self::failed(error),
        }
    }
}

impl From<TransferError> for SyncError {
    fn from(error: TransferError) -> Self {
        match error {
            TransferError::AuthenticationRequired => Self::AuthenticationRequired,
            error @ (TransferError::Retryable(_) | TransferError::Rejected(_)) => Self::failed(error),
        }
    }
}

macro_rules! failed_from {
    ($($error:ty),+ $(,)?) => {$(
        impl From<$error> for SyncError {
            fn from(error: $error) -> Self {
                Self::failed(error)
            }
        }
    )+};
}

failed_from!(
    crate::platform::database::DatabaseError,
    rusqlite::Error,
    std::io::Error,
    std::time::SystemTimeError,
    std::num::TryFromIntError,
    sync_common::PullBatchError,
    sync_common::PushResponseError,
    crate::library::database::OutboxReadError,
);

#[cfg(target_arch = "wasm32")]
impl From<web_time::SystemTimeError> for SyncError {
    fn from(error: web_time::SystemTimeError) -> Self {
        Self::failed(error)
    }
}

impl From<&'static str> for SyncError {
    fn from(message: &'static str) -> Self {
        Self::failed(std::io::Error::other(message))
    }
}
