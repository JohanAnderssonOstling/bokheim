use library_database::DatabaseError;
use std::error::Error;
use std::fmt;
use client_protocol::{BackendError, Classified, Failure};

pub type SyncResult<T> = Result<T, SyncError>;

#[derive(Debug)]
pub enum SyncError {
    AuthenticationRequired,
    Failed(BackendError),
}


impl fmt::Display for SyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AuthenticationRequired => formatter.write_str("synchronization authentication is required"),
            Self::Failed(error) => fmt::Display::fmt(error, formatter),
        }
    }
}

impl Error for SyncError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::AuthenticationRequired => Some(&Failure::SignInRequired),
            Self::Failed(error) => Some(error),
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

failed_from!( &'static str, DatabaseError, rusqlite::Error, std::io::Error, web_time::SystemTimeError, std::num::TryFromIntError,);

impl From<crate::engine::SyncError> for SyncError {
    fn from(error: crate::engine::SyncError) -> Self {
        match error {
            crate::engine::SyncError::AuthenticationRequired => Self::AuthenticationRequired,
            error @ crate::engine::SyncError::CursorRejected => Self::failed(error),
            crate::engine::SyncError::Failed(message) => Self::Failed(message),
        }
    }
}

impl Classified for SyncError {
    fn failure(&self) -> Option<Failure> {
        match self {
            Self::AuthenticationRequired => Some(Failure::SignInRequired),
            Self::Failed(error) => error.failure(),
        }
    }
}

impl From<Box<dyn Error>> for SyncError {
    fn from(error: Box<dyn Error>) -> Self {
        Self::failed(client_protocol::OperationError::from_error(error.as_ref()))
    }
}
