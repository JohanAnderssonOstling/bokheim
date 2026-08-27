use std::future::Future;
use std::pin::Pin;

use bokheim_backend::app::executor::{BackendBlockingOperation, BackendFuture, BackendOutput};
use bokheim_backend::{AppBackend, BackendError, LibraryBackend};

use crate::navigation::LibrarySummary;

pub(crate) type UiFuture<T> = Pin<Box<dyn Future<Output = Result<T, String>> + 'static>>;

pub(crate) fn library_summaries(backend: &AppBackend) -> Result<Vec<LibrarySummary>, String> {
    backend.libraries().map(|entries| entries.into_iter().map(|entry| LibrarySummary::new(*entry.library_id(), entry.library_name())).collect()).map_err(|error| error.to_string())
}

pub(crate) fn ensure_default_library(backend: &AppBackend) -> Result<LibrarySummary, String> {
    backend.ensure_default_library().map(|entry| LibrarySummary::new(*entry.library_id(), entry.library_name())).map_err(|error| error.to_string())
}

pub(crate) fn library_blocking<T, F>(backend: &LibraryBackend, operation: F) -> UiFuture<T>
where
    T: BackendOutput,
    F: BackendBlockingOperation<LibraryBackend, Result<T, BackendError>>,
{
    let task = backend.spawn_blocking(operation);
    Box::pin(async move { task.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
}

pub(crate) fn library_async<T, F, Fut>(backend: &LibraryBackend, operation: F) -> UiFuture<T>
where
    T: BackendOutput,
    F: FnOnce(LibraryBackend) -> Fut,
    Fut: BackendFuture<Result<T, BackendError>>,
{
    let task = backend.spawn(operation);
    Box::pin(async move { task.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
}

pub(crate) fn app_async<T, F, Fut>(backend: &AppBackend, operation: F) -> UiFuture<T>
where
    T: BackendOutput,
    F: FnOnce(AppBackend) -> Fut,
    Fut: BackendFuture<Result<T, BackendError>>,
{
    let task = backend.spawn(operation);
    Box::pin(async move { task.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
}

pub(crate) fn app_blocking<T, F>(backend: &AppBackend, operation: F) -> UiFuture<T>
where
    T: BackendOutput,
    F: BackendBlockingOperation<AppBackend, Result<T, BackendError>>,
    Result<T, BackendError>: BackendOutput,
{
    let task = backend.spawn(move |backend| async move { bokheim_backend::app::executor::run_blocking(move || operation(backend)).await });
    Box::pin(async move { task.await.map_err(|error| error.to_string())?.map_err(|error| error.to_string())?.map_err(|error| error.to_string()) })
}
