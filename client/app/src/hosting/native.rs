//! Native startup and host lifetime; foreground calls use domain clients.
use crate::app::AppBackend;
use crate::{AppClient, AppStartupState, BackendError};
impl AppClient {
    /// Opens startup libraries synchronously, then assembles clients on the shared backend runtime.
    pub fn start_native(launch: crate::api::BackendLaunch) -> Result<(Self, AppStartupState), BackendError> {
        let backend_context = launch.into_context()?;
        let executor = backend_context.executor.clone();
        let backend = executor.block_on(AppBackend::new(backend_context))?;
        let startup = executor.block_on(backend.startup_state())?;
        Ok((super::client(backend, None), startup))
    }
}

#[cfg(test)]
#[path = "../../tests/hosting/native/ordering_tests.rs"]
mod ordering_tests;
