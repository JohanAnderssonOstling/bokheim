//! Shared backend error used by application, library, and sync boundaries.
use std::fmt;

#[derive(Clone, Debug)]
pub struct BackendError(String);

impl BackendError {
    pub(crate) fn message(message: impl Into<String>) -> Self {
        Self(message.into())
    }
    pub(crate) fn operation(error: impl fmt::Display) -> Self {
        Self(error.to_string())
    }
}
impl fmt::Display for BackendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl std::error::Error for BackendError {}

/// Wire boundaries report failures as plain strings. Command dispatch relies on
/// this so one `?` handles both string-erroring and `BackendError` operations.
impl From<BackendError> for String {
    fn from(error: BackendError) -> Self {
        error.0
    }
}

macro_rules! backend_error_from {
    ($($error:ty),+ $(,)?) => {$ (
        impl From<$error> for BackendError {
            fn from(error: $error) -> Self { Self::operation(error) }
        }
    )+};
}
backend_error_from!(
    String,
    &str,
    std::io::Error,
    rusqlite::Error,
    Box<dyn std::error::Error>,
    Box<dyn std::error::Error + Send + Sync>,
    crate::executor::BackendExecutorError,
    std::time::SystemTimeError,
    crate::storage::database::DatabaseError,
    crate::storage::files::AssetStoreError,
);
