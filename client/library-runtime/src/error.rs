//! Shared error used at application and library ownership boundaries.

use std::fmt;

#[derive(Clone, Debug)]
pub struct BackendError(String);

impl BackendError {
    pub fn message(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn operation(error: impl fmt::Display) -> Self {
        Self(error.to_string())
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for BackendError {}

/// Wire boundaries report failures as plain strings. Command dispatch relies
/// on this so one `?` handles both string-erroring and `BackendError` work.
impl From<BackendError> for String {
    fn from(error: BackendError) -> Self {
        error.0
    }
}

macro_rules! backend_error_from {
    ($($error:ty),+ $(,)?) => {$ (
        impl From<$error> for BackendError {
            fn from(error: $error) -> Self {
                Self::operation(error)
            }
        }
    )+};
}

backend_error_from!(
    String,
    &str,
    std::io::Error,
    library_database::rusqlite::Error,
    Box<dyn std::error::Error>,
    Box<dyn std::error::Error + Send + Sync>,
    client_platform_runtime::executor::BackendExecutorError,
    std::time::SystemTimeError,
    library_database::DatabaseError,
);
