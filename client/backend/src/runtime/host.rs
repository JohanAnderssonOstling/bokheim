//! Restricted entry point for a platform that hosts the browser worker.
use super::{AppStartupState, AppWorkerRequest, WorkerDispatchResult};
use crate::app::{AppBackend, AppDataLocation};

#[derive(Clone)]
pub struct BackendWorkerHost(AppBackend);

impl BackendWorkerHost {
    pub async fn initialize(location: AppDataLocation) -> Result<Self, String> {
        crate::initialize_web_storage().await?;
        let backend = AppBackend::initialize(location).map_err(|error| error.to_string())?;
        Ok(Self(backend))
    }

    pub fn startup_state(&self) -> Result<AppStartupState, String> {
        super::app_startup_state(&self.0)
    }

    pub async fn dispatch(&self, request: AppWorkerRequest) -> Result<WorkerDispatchResult, String> {
        super::dispatch_worker_request(self.0.clone(), request).await
    }
}
