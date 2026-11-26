//! Native startup and host lifetime; foreground calls use domain clients.
use crate::app::AppBackend;
use crate::{AppClient, AppStartupState, BackendError};
impl AppClient {
    /// Assemble clients from private startup state. Android opens library files in the background.
    pub fn start_native(launch: crate::api::BackendLaunch) -> Result<(Self, AppStartupState), BackendError> {
        let backend_context = launch.into_context()?;
        let executor = backend_context.executor.clone();
        let backend = executor.block_on(AppBackend::new(backend_context))?;
        let startup = executor.block_on(backend.startup_state())?;
        #[cfg(target_os = "android")]
        {
            let opening = backend.clone();
            executor.spawn_detached(async move {
                if let Err(error) = opening.open_startup_libraries().await {
                    log::warn!("Could not open startup libraries: {error}");
                }
            });
        }
        Ok((super::client(backend, None), startup))
    }
}

#[cfg(test)]
#[path = "../../tests/hosting/native/ordering_tests.rs"]
mod ordering_tests;
