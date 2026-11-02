use crate::app::AppBackend;
use crate::host::startup_profile::StartupProfile;
use crate::services::Services;
use crate::BackendError;
use client_platform_runtime::storage_queue::{self, Priority};
use client_runtime::reply::Reply;
use futures_util::future::LocalBoxFuture;
use futures_util::stream::FuturesUnordered;
use library_backend::account::Client as AccountClient;
use library_backend::management::ManagementClient;
use std::sync::Arc;
use tokio::sync::watch;
#[derive(Clone)]
pub(crate) struct Transport {
    jobs: async_channel::Sender<BackendRequest>,
}

use super::*;
use futures_util::{FutureExt, StreamExt};
use std::panic::AssertUnwindSafe;

fn request_panicked(_: Box<dyn std::any::Any + Send>) -> BackendError {
    log::error!("backend request panicked; request failed without stopping the worker");
    BackendError::message("backend request failed unexpectedly")
}

type CommandResult = Result<WorkerDispatchResult<Reply>, BackendError>;

struct BackendRequest {
    command: AppCommand,
    reply: oneshot::Sender<CommandResult>,
}

/// Keep the large dispatcher future inside the worker, independent of reply types.
async fn execute_command(backend: AppBackend, command: AppCommand, reply: oneshot::Sender<CommandResult>) {
    // Retain the sender outside the panic boundary, including future construction.
    // A failed write is never retried: it may already have committed.
    let operation = async move { super::requests::dispatch_request(backend, command).await };
    let result = AssertUnwindSafe(operation).catch_unwind().await.map_err(request_panicked).and_then(|result| result);
    let _ = reply.send(result);
}

pub(crate) struct AppWorker {
    jobs: async_channel::Receiver<BackendRequest>,
    library_startup: watch::Sender<Option<Result<(), BackendError>>>,
    initialize_libraries: bool,
}

pub(crate) fn channel(backend: &AppBackend) -> (AppClient, AppWorker) {
    channel_with_startup(backend, false)
}

fn channel_with_startup(backend: &AppBackend, initialize_libraries: bool) -> (AppClient, AppWorker) {
    let (jobs, incoming) = async_channel::bounded(64);
    let (library_startup, ready) = watch::channel((!initialize_libraries).then_some(Ok(())));
    let management = ManagementClient::new(backend.state.library_service.clone(), ready.clone(), jobs.clone());
    let account = AccountClient::local(backend.state.account_service.clone(), management.clone());
    (AppClient { account, management, transport: Transport { jobs }, services: Arc::new(Services::new(backend.state.executor.clone())) }, AppWorker { jobs: incoming, library_startup, initialize_libraries })
}

impl AppWorker {
    /// Deferred startup opens every library before serving queued commands.
    /// Settings then run in receive order; waiting operations run concurrently.
    pub async fn serve(self, backend: AppBackend) {
        if self.initialize_libraries {
            crate::executor::library_maintenance_ready().await;
            let initializer = backend.clone();
            let result = storage_queue::run(Priority::Background, move || initializer.open_startup_libraries()).await.map_err(BackendError::message).and_then(|result| result);
            self.library_startup.send_replace(Some(result.clone()));
            if let Err(error) = result {
                log::error!("could not open startup libraries: {error}");
                return;
            }
        }
        let mut running: FuturesUnordered<LocalBoxFuture<'static, ()>> = FuturesUnordered::new();
        loop {
            tokio::select! {
                job = self.jobs.recv() => match job {
                    Ok(BackendRequest { command, reply }) => {
                        if command.is_settings_update() {
                            let result = AssertUnwindSafe(super::requests::execute_ordered(&backend, command)).catch_unwind().await
                                .map_err(request_panicked)
                                .and_then(|result| result)
                                .map(WorkerDispatchResult::Response);
                            let _ = reply.send(result);
                        } else {
                            running.push(Box::pin(execute_command(backend.clone(), command, reply)));
                        }
                    }
                    Err(_) => break,
                },
                Some(()) = running.next(), if !running.is_empty() => {},
            }
        }
        while running.next().await.is_some() {}
    }
}

impl Transport {
    async fn command(&self, command: AppCommand) -> CommandResult {
        let (reply, result) = oneshot::channel();
        self.jobs.send(BackendRequest { command, reply }).await.map_err(|_| "backend worker stopped during request".to_owned())?;
        result.await.map_err(|_| "backend worker stopped before replying".to_owned())?
    }

    pub(crate) async fn request<T: WireReply>(&self, command: AppCommand) -> Result<T, BackendError> {
        match self.command(command).await? {
            WorkerDispatchResult::Response(value) => value.take(),
            WorkerDispatchResult::Subscription { .. } => Err("backend returned a subscription for a request".into()),
        }
    }
}

impl AppClient {
    /// Loads startup metadata and starts the application-owned backend thread.
    /// Hosts may defer the library-opening phase until their first frame.
    pub fn start_native(launch: crate::api::BackendLaunch) -> Result<(Self, AppStartupState), BackendError> {
        let mut profile = StartupProfile::new("native_backend");
        let defer_libraries = launch.libraries_after_first_frame;
        let backend_context = launch.into_context()?;
        profile.mark("registry_open");
        let backend = if defer_libraries { AppBackend::new_without_libraries(backend_context) } else { AppBackend::new(backend_context) }.map_err(BackendError::operation)?;
        profile.mark("backend_construct");
        let startup = backend.startup_state()?;
        profile.mark("startup_snapshot");
        let (client, worker) = if defer_libraries { channel_with_startup(&backend, true) } else { channel(&backend) };
        std::thread::Builder::new().name("bokheim-app-worker".to_owned()).spawn(move || backend.run_native_worker(worker)).map_err(|error| format!("failed to start application backend worker: {error}"))?;
        profile.mark("worker_start");
        Ok((client, startup))
    }
}

#[cfg(test)]
#[path = "../../tests/runtime/native/ordering_tests.rs"]
mod ordering_tests;
