use library_database::DatabaseError;
use std::error::Error;
use std::fmt::Display;

pub type SyncResult<T> = Result<T, SyncError>;

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("synchronization authentication is required")]
    AuthenticationRequired,
    #[error("{0}")]
    Failed(String),
}

impl SyncError {
    pub fn failed(error: impl Display) -> Self {
        Self::Failed(error.to_string())
    }
}

impl From<sync_transport::SyncRequestError> for SyncError {
    fn from(error: sync_transport::SyncRequestError) -> Self {
        crate::engine::SyncError::from(error).into()
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

failed_from!(Box<dyn Error>, &'static str, DatabaseError, rusqlite::Error, std::io::Error, web_time::SystemTimeError, std::num::TryFromIntError,);

impl From<crate::engine::SyncError> for SyncError {
    fn from(error: crate::engine::SyncError) -> Self {
        match error {
            crate::engine::SyncError::AuthenticationRequired => Self::AuthenticationRequired,
            error @ crate::engine::SyncError::CursorRejected => Self::failed(error),
            crate::engine::SyncError::Failed(message) => Self::Failed(message),
        }
    }
}
