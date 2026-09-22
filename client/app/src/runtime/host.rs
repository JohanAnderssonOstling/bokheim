//! Restricted entry point for a platform that hosts the browser worker.
use super::{AppStartupState, AppWorkerRequest, WorkerDispatchResult};
use crate::app::{AppBackend, AppDataLocation};

#[derive(Clone)]
pub struct BackendWorkerHost(AppBackend);

impl BackendWorkerHost {
    pub async fn initialize(location: AppDataLocation) -> Result<Self, crate::BackendError> {
        crate::initialize_web_storage().await?;
        let backend = AppBackend::initialize(location).map_err(crate::BackendError::operation)?;
        Ok(Self(backend))
    }

    pub fn startup_state(&self) -> Result<AppStartupState, crate::BackendError> {
        super::app_startup_state(&self.0)
    }

    pub async fn dispatch(&self, request: AppWorkerRequest) -> Result<WorkerDispatchResult, crate::BackendError> {
        super::dispatch_worker_request(self.0.clone(), request).await
    }
}
