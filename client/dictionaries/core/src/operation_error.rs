pub use operation_failure::OperationError;
pub type OperationResult<T> = Result<T, OperationError>;
