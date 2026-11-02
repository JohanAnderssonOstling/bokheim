//! UI-owned browser launching; the selected library owns downloads and imports.
use app_preferences::BrowsingPreferences;
use library_backend::{BackendError, LibraryClient};
#[cfg(not(target_arch = "wasm32"))]
use library_backend::{
    Url,
    executor::{BackendExecutor, BackendExecutorError, run_blocking},
};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::LazyLock;

#[cfg(not(target_arch = "wasm32"))]
fn executor() -> Result<&'static BackendExecutor, BackendError> {
    static EXECUTOR: LazyLock<Result<BackendExecutor, BackendExecutorError>> = LazyLock::new(BackendExecutor::new);
    EXECUTOR.as_ref().map_err(|error| BackendError::message(error.to_string()))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn available() -> bool {
    let Ok(executor) = executor() else { return false };
    executor.spawn(async { run_blocking(embedded_browser::available).await.unwrap_or(false) }).await.unwrap_or(false)
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn available() -> bool {
    false
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn open(library: LibraryClient, preferences: BrowsingPreferences) -> Result<(), BackendError> {
    executor()?
        .spawn(async move {
            let downloads = library.browser_downloads().await?;
            let url = Url::parse("about:blank").map_err(BackendError::operation)?;
            let window = embedded_browser::open(&url, "Bokheim – Browser", downloads.directory(), preferences).await?;
            downloads.attach(window);
            Ok(())
        })
        .await
        .map_err(BackendError::operation)?
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn open(_library: LibraryClient, _preferences: BrowsingPreferences) -> Result<(), BackendError> {
    Err(BackendError::message("the embedded browser is unavailable on this platform"))
}
