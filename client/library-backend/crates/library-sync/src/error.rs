use client_protocol::{BackendError, Classified, Failure};
use library_database::DatabaseError;
use std::error::Error;
use std::fmt;

pub type SyncResult<T> = Result<T, SyncError>;

#[derive(Debug)]
pub enum SyncError {
    AuthenticationRequired,
    CursorRejected,
    Failed(BackendError),
}

impl fmt::Display for SyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationRequired => formatter.write_str("synchronization authentication is required"),
            Self::Failed(error) => fmt::Display::fmt(error, formatter),
            Self::CursorRejected => formatter.write_str("the synchronization cursor was rejected after recovery"),
        }
    }
}

impl Error for SyncError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::AuthenticationRequired => Some(&Failure::SignInRequired),
            Self::Failed(error) => Some(error),
            Self::CursorRejected => None,
        }
    }
}

impl SyncError {
    pub fn failed(error: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        Self::Failed(BackendError::operation(error))
    }
}

impl From<sync_transport::SyncRequestError> for SyncError {
    fn from(error: sync_transport::SyncRequestError) -> Self {
        match error {
            sync_transport::SyncRequestError::AuthenticationRequired => Self::AuthenticationRequired,
            sync_transport::SyncRequestError::CursorRejected => Self::CursorRejected,
            error => Self::failed(error),
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

failed_from!(&'static str, DatabaseError, rusqlite::Error, std::io::Error, web_time::SystemTimeError, std::num::TryFromIntError,);

impl Classified for SyncError {
    fn failure(&self) -> Option<Failure> {
        match self {
            Self::AuthenticationRequired => Some(Failure::SignInRequired),
            Self::Failed(error) => error.failure(),
            Self::CursorRejected => None,
        }
    }
}

impl From<Box<dyn Error>> for SyncError {
    fn from(error: Box<dyn Error>) -> Self {
        Self::failed(client_protocol::OperationError::from_error(error.as_ref()))
    }
}
