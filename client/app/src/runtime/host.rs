//! Restricted entry point for a platform that hosts the browser worker.

use super::{AppStartupState, AppWorkerRequest, WorkerDispatchResult};
use crate::app::{AppBackend, AppDataLocation};
use crate::BackendError;

#[derive(Clone)]
pub struct BackendWorkerHost(AppBackend);

impl BackendWorkerHost {
    /// Browser startup always uses the deployed bundle and its bundled taxonomy.
    /// Finish migrations before opening sessions; failures propagate to the page.
    pub async fn initialize(location: AppDataLocation) -> Result<Self, BackendError> {
        crate::initialize_web_storage().await?;
        let context = crate::app::BackendContext::initialize(location).map_err(BackendError::message)?;
        let entries = context.registry.entries().map_err(BackendError::operation)?;
        for entry in &entries {
            let locator = context.registry.library_database_locator(entry.library_id(), entry.storage_locator());
            let db = library_database::Database::open(&locator).map_err(BackendError::operation)?;
            db.initialize_library().map_err(BackendError::operation)?;
        }
        let backend = AppBackend::new(context)?;
        for entry in entries {
            backend.library_directory().get(entry.library_id())?;
        }
        Ok(Self(backend))
    }

    pub fn startup_state(&self) -> Result<AppStartupState, BackendError> {
        super::app_startup_state(&self.0)
    }

    pub async fn dispatch(&self, request: AppWorkerRequest) -> Result<WorkerDispatchResult, BackendError> {
        super::dispatch_worker_request(self.0.clone(), request).await
    }
}
