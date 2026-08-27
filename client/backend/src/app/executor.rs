//! Target-selected async executor. Backend operations use this one boundary;
//! native hosts get Tokio's thread pool and browser hosts get the local event
//! loop required by Fetch and single-connection SQLite.

use std::fmt;
use std::future::Future;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendExecutorError(String);

impl fmt::Display for BackendExecutorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for BackendExecutorError {}

#[cfg(not(target_arch = "wasm32"))]
pub trait BackendSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + ?Sized> BackendSend for T {}

#[cfg(target_arch = "wasm32")]
pub trait BackendSend {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> BackendSend for T {}

pub trait BackendOutput: BackendSend + 'static {}
impl<T: BackendSend + 'static> BackendOutput for T {}

pub trait BackendFuture<T>: Future<Output = T> + BackendSend + 'static {}
impl<T, F: Future<Output = T> + BackendSend + 'static> BackendFuture<T> for F {}

pub trait BackendBlockingOperation<Argument, Output>: FnOnce(Argument) -> Output + BackendSend + 'static {}
impl<Argument, Output, F> BackendBlockingOperation<Argument, Output> for F where F: FnOnce(Argument) -> Output + BackendSend + 'static {}

pub trait BackendBlockingThunk<Output>: FnOnce() -> Output + BackendSend + 'static {}
impl<Output, F> BackendBlockingThunk<Output> for F where F: FnOnce() -> Output + BackendSend + 'static {}

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::*;
