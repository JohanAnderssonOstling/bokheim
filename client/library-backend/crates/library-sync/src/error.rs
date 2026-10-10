use std::error::Error;
use std::fmt::Display;
use asset_transfer::TransferError;
use library_database::DatabaseError;

pub type SyncResult<T> = Result<T, SyncError>;

#[derive(Debug)]
pub enum SyncError {
    AuthenticationRequired,
    Failed(String),
}

impl SyncError {
    pub fn failed(error: impl Display) -> Self {
        Self::Failed(error.to_string())
    }
}

impl Display for SyncError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthenticationRequired => formatter.write_str("synchronization authentication is required"),
            Self::Failed(error) => error.fmt(formatter),
        }
    }
}

impl Error for SyncError {}

impl From<sync_transport::SyncRequestError> for SyncError {
    fn from(error: sync_transport::SyncRequestError) -> Self {
        crate::engine::SyncError::from(error).into()
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

failed_from!(Box<dyn Error>, &'static str, DatabaseError, library_files::asset_store::AssetStoreError, rusqlite::Error, std::io::Error, web_time::SystemTimeError, std::num::TryFromIntError,);

impl From<crate::engine::SyncError> for SyncError {
    fn from(error: crate::engine::SyncError) -> Self {
        match error {
            crate::engine::SyncError::AuthenticationRequired => Self::AuthenticationRequired,
            error @ crate::engine::SyncError::CursorRejected => Self::failed(error),
            crate::engine::SyncError::Failed(message) => Self::Failed(message),
        }
    }
}
