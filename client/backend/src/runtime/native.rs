#[derive(Clone)]
pub(crate) struct Transport {
    jobs: async_channel::Sender<BackendJob>,
}

use super::*;
use futures_util::{FutureExt, StreamExt};
use std::panic::AssertUnwindSafe;

fn request_panicked(_: Box<dyn std::any::Any + Send>) -> String {
    log::error!("backend request panicked; request failed without stopping the worker");
    "backend request failed unexpectedly".to_owned()
}

#[cfg(test)]
mod panic_tests {
    use super::*;

    #[tokio::test]
    async fn request_panics_preserve_worker_and_other_pending_requests() {
        let root = tempfile::tempdir().unwrap();
        let context = crate::BackendContext::initialize(crate::AppDataLocation::native_path(root.path())).unwrap();
        let backend = AppBackend::new(context).unwrap();
        let (client, worker) = channel();
        let exercise = async move {
            let (started, ready) = oneshot::channel();
            let (release, resume) = oneshot::channel();
            let pending = client.transport.run(move |_| async move {
                started.send(()).unwrap();
                resume.await.unwrap();
                Ok(42)
            });
            let failures = async {
                ready.await.unwrap();
                let construction = client.transport.run::<(), _, std::future::Ready<Result<(), String>>>(|_| panic!("construction failure")).await;
                assert_eq!(construction.unwrap_err(), "backend request failed unexpectedly");
                let polling = client
                    .transport
                    .run::<(), _, _>(|_| async {
                        tokio::task::yield_now().await;
                        panic!("polling failure");
                    })
                    .await;
                assert_eq!(polling.unwrap_err(), "backend request failed unexpectedly");
                assert_eq!(client.transport.run_ordered::<(), _>(|_| panic!("ordered failure")).await.unwrap_err(), "backend request failed unexpectedly");
                assert_eq!(client.transport.run_ordered(|_| Ok(7)).await.unwrap(), 7);
                assert_eq!(client.transport.run::<(), _, _>(|_| async { Err("ordinary error".into()) }).await.unwrap_err(), "ordinary error");
                release.send(()).unwrap();
            };
            let (result, ()) = tokio::join!(pending, failures);
            assert_eq!(result.unwrap(), 42);
            drop(client);
        };
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(worker.serve(backend), exercise);
        })
        .await
        .expect("worker must survive failures and drain on shutdown");
    }
}

type NativeJobFuture = std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>;

pub(crate) enum BackendJob {
    Run(Box<dyn FnOnce(AppBackend) -> NativeJobFuture + Send>),
    RunOrdered(Box<dyn FnOnce(&AppBackend) + Send>),
}

pub(crate) struct AppWorker {
    jobs: async_channel::Receiver<BackendJob>,
}

pub(crate) fn channel() -> (AppClient, AppWorker) {
    let (jobs, incoming) = async_channel::bounded(64);
    (AppClient { transport: Transport { jobs } }, AppWorker { jobs: incoming })
}

impl AppWorker {
    /// Settings run in receive order. Waiting requests must not prevent local
    /// commands from running; resource limits belong to the work they protect.
    pub async fn serve(self, backend: AppBackend) {
        let mut running = futures_util::stream::FuturesUnordered::new();
        loop {
            tokio::select! {
                job = self.jobs.recv() => match job {
                    Ok(BackendJob::RunOrdered(operation)) => operation(&backend),
                    Ok(BackendJob::Run(operation)) => {
                        let backend = backend.clone();
                        running.push(async move {
                            operation(backend).await;
                        });
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
    pub(crate) async fn run<T, F, Fut>(&self, operation: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(AppBackend) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T, String>> + 'static,
    {
        let (response, result) = oneshot::channel();
        let operation = Box::new(move |backend| {
            Box::pin(async move {
                // Keep construction and polling inside the boundary, while
                // retaining the reply sender outside it. Never retry a write
                // automatically: it may have committed before panicking.
                let result = AssertUnwindSafe(async move { operation(backend).await }).catch_unwind().await.map_err(request_panicked).and_then(|result| result);
                let _ = response.send(result);
            }) as NativeJobFuture
        });
        self.jobs.send(BackendJob::Run(operation)).await.map_err(|_| "backend worker stopped during request".to_owned())?;
        result.await.map_err(|_| "backend worker stopped before replying".to_owned())?
    }

    pub(crate) async fn run_ordered<T, F>(&self, operation: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&AppBackend) -> Result<T, String> + Send + 'static,
    {
        let (response, result) = oneshot::channel();
        self.jobs
            .send(BackendJob::RunOrdered(Box::new(move |backend| {
                let result = std::panic::catch_unwind(AssertUnwindSafe(|| operation(backend))).map_err(request_panicked).and_then(|result| result);
                let _ = response.send(result);
            })))
            .await
            .map_err(|_| "backend worker stopped during request".to_owned())?;
        result.await.map_err(|_| "backend worker stopped before replying".to_owned())?
    }
}

impl Transport {
    pub(crate) async fn request<T: LibraryReply>(&self, command: AppCommand) -> Result<T, String> {
        if command.is_settings_update() {
            return self.run_ordered(move |backend| super::requests::execute_ordered(backend, command)).await.and_then(decode_worker_payload);
        }
        self.run(move |backend| async move {
            match super::requests::dispatch_request(backend, command).await? {
                WorkerDispatchResult::Response(value) => Ok(value),
                WorkerDispatchResult::Subscription { .. } => Err("backend returned a subscription for a request".into()),
            }
        })
        .await
        .and_then(decode_worker_payload)
    }

    async fn subscribe<T: LibraryReply>(&self, command: AppCommand) -> Result<(WorkerSubscription, T), String> {
        self.run(move |backend| async move {
            match super::requests::dispatch_request(backend, command).await? {
                WorkerDispatchResult::Subscription { initial, events } => Ok((events, initial)),
                WorkerDispatchResult::Response(_) => Err("backend returned a response for a subscription".into()),
            }
        })
        .await
        .and_then(|(events, initial)| Ok((events, decode_worker_payload(initial)?)))
    }

    pub(super) async fn unit_subscription(&self, command: AppCommand) -> Result<async_channel::Receiver<()>, String> {
        let (WorkerSubscription::Unit(events), ()) = self.subscribe::<()>(command).await? else { return Err("unexpected subscription type".into()) };
        Ok(events)
    }

    pub(crate) async fn library_updates_transport(&self, library_id: LibraryId) -> Result<(async_channel::Receiver<library_model::LibraryUpdate>, bool), String> {
        let (WorkerSubscription::LibraryUpdates(events), initial) = self.subscribe_library(library_id, LibrarySubscription::Updates).await? else { return Err("unexpected subscription type".to_owned()) };
        Ok((events, initial))
    }
    pub(crate) async fn book_download_changes(&self, library_id: LibraryId, content_hash: crate::ContentHash) -> Result<(async_channel::Receiver<crate::DownloadState>, crate::DownloadState), String> {
        let (WorkerSubscription::DownloadStates(events), initial) = self.subscribe_library(library_id, LibrarySubscription::BookDownload { content_hash }).await? else { return Err("unexpected subscription type".to_owned()) };
        Ok((events, initial))
    }

    async fn subscribe_library<T: serde::de::DeserializeOwned + Send + 'static>(&self, library_id: LibraryId, subscription: LibrarySubscription) -> Result<(WorkerSubscription, T), String> {
        self.run(move |backend| async move {
            match dispatch_worker_request(backend, AppWorkerRequest { id: 0, command: WorkerCommand::LibrarySubscription { library_id, subscription } }).await? {
                WorkerDispatchResult::Subscription { initial, events } => Ok((events, decode_worker_payload(initial)?)),
                WorkerDispatchResult::Response(_) => Err("library subscription returned a response".into()),
            }
        })
        .await
    }
}

impl AppClient {
    /// Starts the native backend on its application-owned thread.
    pub fn start_native(launch: crate::api::BackendLaunch) -> Result<(Self, AppStartupState), String> {
        let backend_context = launch.into_context()?;
        let backend = AppBackend::new(backend_context).map_err(|error| error.to_string())?;
        let startup = app_startup_state(&backend)?;
        let (client, worker) = crate::runtime::native::channel();
        std::thread::Builder::new().name("bokheim-app-worker".to_owned()).spawn(move || backend.run_native_worker(worker)).map_err(|error| format!("failed to start application backend worker: {error}"))?;
        Ok((client, startup))
    }
}

impl Transport {
    pub(crate) async fn library_request<T: LibraryReply, C: LibraryRequest>(&self, library_id: LibraryId, command: C) -> Result<T, String> {
        self.run(move |backend| async move {
            let handle = backend.library_directory().get(&library_id).map_err(|error| error.to_string())?;
            handle.dispatch(command.command()).await
        })
        .await
        .and_then(decode_worker_payload)
    }
}

impl crate::LibraryClient {
    pub(crate) async fn run<T, F, Fut>(&self, operation: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(crate::library::LibraryHandle) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T, String>> + 'static,
    {
        let library_id = self.library_id;
        self.app
            .transport
            .run(move |backend| async move {
                let library = backend.library_directory().get(&library_id).map_err(|error| error.to_string())?;
                operation(library).await
            })
            .await
    }
}

impl Transport {
    pub(crate) async fn import_selected_directory(&self, library: &crate::LibraryClient, parent_id: crate::DirId, directory: crate::DirectoryImport, create_root: bool) -> Result<Vec<crate::ImportFailure>, String> {
        let result = library.run(move |library| async move { library.import_selected_directory(parent_id, directory, create_root).await }).await;
        result
    }

    pub(crate) async fn resolve_book(&self, library: &crate::LibraryClient, content_hash: crate::ContentHash) -> Result<crate::ResolvedBook, String> {
        let library_id = library.library_id;
        self.run(move |backend| async move {
            let library = backend.library_directory().get(&library_id).map_err(|error| error.to_string())?;
            library.resolve_book(content_hash).await.map_err(|error| error.to_string())
        })
        .await
    }
}

impl crate::LibraryClient {
    pub async fn import_owned_book(&self, parent_id: crate::DirId, file_name: String, path: std::path::PathBuf) -> Result<crate::ContentHash, String> {
        self.run(move |library| async move { library.import_book_path(parent_id, file_name, path).await.map_err(|error| error.to_string()) }).await
    }
}

/// Native transport has no browser-worker capability commands.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum NoHostCommand {}
impl NoHostCommand {
    pub(crate) fn is_book_subscription(&self) -> bool {
        match *self {}
    }
    pub(crate) async fn dispatch_app(self, _: AppBackend) -> Result<WorkerDispatchResult, String> {
        match self {}
    }
}
