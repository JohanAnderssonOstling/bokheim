//! Restricted entry point for a platform that hosts the browser worker.

use super::requests::dispatch_request;
use super::{AppStartupState, AppWorkerRequest, WorkerCommand, WorkerDispatchResult, WorkerSubscription};
use crate::app::{AppBackend, AppDataLocation};
use crate::BackendError;
use client_runtime::reply::Reply;
use library_backend::management::ManagementSubscription;

#[derive(Clone)]
pub struct BackendWorkerHost(AppBackend);

impl BackendWorkerHost {
    /// Browser startup always uses the deployed bundle and the taxonomy the
    /// update manifest publishes, installed before any library opens.
    /// Finish migrations before opening sessions; failures propagate to the page.
    pub async fn initialize(location: AppDataLocation) -> Result<Self, BackendError> {
        crate::initialize_web_storage().await.map_err(|error| match error {
            crate::platform::web_storage::StorageInitError::Busy(error) => BackendError::classified(crate::Failure::StorageBusy, error),
            crate::platform::web_storage::StorageInitError::Failed(error) => BackendError::message(error),
        })?;
        super::bisac::install().await.map_err(BackendError::message)?;
        super::taxonomy::install().await.map_err(BackendError::message)?;
        let context = crate::app::BackendContext::initialize(location).await.map_err(BackendError::message)?;
        let entries = context.registry.entries().map_err(BackendError::operation)?;
        for entry in &entries {
            let locator = context.registry.library_database_locator(entry.library_id(), entry.storage_locator());
            let db = library_database::Database::open(&locator).map_err(BackendError::operation)?;
            db.initialize_library_for(*entry.library_id()).map_err(BackendError::operation)?;
        }
        let backend = AppBackend::new(context)?;
        for entry in entries {
            backend.library_directory().get(entry.library_id())?;
        }
        Ok(Self(backend))
    }

    pub fn startup_state(&self) -> Result<AppStartupState, BackendError> {
        self.0.startup_state()
    }

    pub async fn dispatch(&self, request: AppWorkerRequest) -> Result<WorkerDispatchResult, BackendError> {
        match request.command {
            WorkerCommand::App(command) => dispatch_request(self.0.clone(), command).await?.into_wire(),
            WorkerCommand::Account(command) => Ok(WorkerDispatchResult::Response(super::encode_reply(self.0.state.account_service.dispatch(command).await?)?)),
            WorkerCommand::Management(command) => Ok(WorkerDispatchResult::Response(self.0.state.library_service.dispatch(command).await?.into_wire()?)),
            WorkerCommand::ManagementSubscription(subscription) => {
                let service = &self.0.state.library_service;
                let events = match subscription {
                    ManagementSubscription::LibraryList => WorkerSubscription::LibraryList(service.list_changes()),
                    ManagementSubscription::Notifications { browsing } => WorkerSubscription::Unit(service.notification_subscription(browsing)),
                };
                Ok(WorkerDispatchResult::Subscription { initial: Reply::new(()).into_wire()?, events })
            }
            WorkerCommand::Library { library_id, command } => {
                let handle = self.0.library_directory().get(&library_id).map_err(BackendError::operation)?;
                Ok(WorkerDispatchResult::Response(handle.dispatch(command).await?.into_wire()?))
            }
            WorkerCommand::LibrarySubscription { library_id, subscription } => {
                let handle = self.0.library_directory().get(&library_id).map_err(BackendError::operation)?;
                let (initial, events) = handle.subscribe(subscription).await?;
                Ok(WorkerDispatchResult::Subscription { initial: initial.into_wire()?, events: WorkerSubscription::Library(events) })
            }
        }
    }
}
