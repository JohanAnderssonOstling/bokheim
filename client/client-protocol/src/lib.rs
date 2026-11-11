//! Common client errors and versioned diagnostic encoding.
mod error;
pub mod wire;
pub use error::{BackendError, Classified, ErrorCause, ErrorReport, Failure, OperationError, WifiFailure};
