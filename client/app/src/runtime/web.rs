use crate::executor::{spawn_detached, BackendExecutor, BoxedBackendFuture};
use crate::services::Services;
use crate::web::WebConnection;
use crate::BackendError;
use async_channel::{bounded, unbounded, Receiver, Sender, WeakSender};
use library_backend::account::{AccountSnapshot, Client as AccountClient, Command as AccountCommand, Transport as AccountTransport};
use library_backend::client::{ClientTransport, TransportResult};
use library_backend::management::{ManagementClient, ManagementCommand, ManagementSubscription, ManagementTransport};
use library_backend::{BookId, DirId, DirectoryImport, ImportFailure, LibraryClient, LibraryCommand, LibraryId, ResolvedBookData};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::watch;

use super::web_state::{Reply, WebState};
use super::*;
use client_platform_web::transport::{
    error_message, field,
    session::{Session, SessionMessage, SessionSender},
};

#[derive(Clone)]
pub(crate) struct Transport {
    remote: Rc<WebWorkerClient>,
}

struct WebPendingRequest<'a> {
    client: &'a WebWorkerClient,
    id: u64,
    subscription: bool,
}

impl Drop for WebPendingRequest<'_> {
    fn drop(&mut self) {
        let _ = self.client.commands.try_send(Command::Cancel { id: self.id, subscription: self.subscription });
    }
}

enum Command {
    Request { id: u64, bytes: Vec<u8>, reply: Reply, events: Option<Sender<WorkerPayload>>, restore: bool, files: Option<(String, js_sys::Array)> },
    Cancel { id: u64, subscription: bool },
    Message(SessionMessage),
    Failed(BackendError),
}

pub(crate) struct WebWorkerClient {
    commands: Sender<Command>,
    next: Cell<u64>,
}

impl Drop for WebWorkerClient {
    fn drop(&mut self) {
        // Discard queued work and release the session when the last client
        // goes away. Session callbacks hold only weak senders.
        self.commands.close();
    }
}

async fn serve_connection(connection: WebConnection, commands: Receiver<Command>, callbacks: WeakSender<Command>, startup: oneshot::Sender<Result<AppStartupState, BackendError>>) {
    let port = SessionSender::new(connection.worker.clone(), connection.port());
    let errors = callbacks.clone();
    let _session = Session::new(
        port.clone(),
        move |message| {
            if let Some(commands) = callbacks.upgrade() {
                let _ = commands.try_send(Command::Message(message));
            }
        },
        move |error| {
            if let Some(commands) = errors.upgrade() {
                let _ = commands.try_send(Command::Failed(error.into()));
            }
        },
    );
    let _connection = connection;
    let mut state = WebState::new(startup);
    while let Ok(command) = commands.recv().await {
        if commands.is_closed() {
            break;
        }
        match command {
            Command::Request { id, bytes, reply, events, restore, files } => {
                let restore = (restore && events.is_some()).then(|| bytes.clone());
                state.request(id, reply, events, restore, || match files {
                    Some((token, files)) => port.send_with_files(&bytes, &token, &files),
                    None => port.send(&bytes),
                });
            }
            Command::Cancel { id, subscription } => {
                state.cancel(id, subscription);
                if subscription {
                    if let Ok(bytes) = encode_worker_message(&AppWorkerClientMessage::CancelSubscription { id }) {
                        let _ = port.send(&bytes);
                    }
                }
            }
            Command::Message(SessionMessage::Reconnecting) => state.reconnect(),
            Command::Message(SessionMessage::Failed { error, storage_busy: true }) => state.fail(BackendError::classified(crate::Failure::StorageBusy, error)),
            Command::Message(SessionMessage::Failed { error, storage_busy: false }) => state.fail(error.into()),
            Command::Message(SessionMessage::Bytes(bytes)) => match decode_worker_message(&bytes) {
                Ok(message) => state.receive(message, |bytes| port.send(bytes)),
                Err(error) => state.fail(error.context("invalid backend response")),
            },
            Command::Failed(error) => state.fail(error),
        }
        if state.failed() {
            port.close();
        }
    }
}

impl Transport {
    pub(crate) async fn connect_web_transport(config: crate::web::WebConfiguration) -> Result<(Self, AppStartupState), BackendError> {
        let connection = WebConnection::connect_prestarted(config).await?;
        Self::finish_web_connection(connection).await
    }

    async fn finish_web_connection(connection: WebConnection) -> Result<(Self, AppStartupState), BackendError> {
        let failure = connection.startup_failure();
        if !failure.is_undefined() {
            let error = error_message(&field(&failure, "error"));
            return Err(if field(&failure, "storageBusy").as_bool() == Some(true) { BackendError::classified(crate::Failure::StorageBusy, error) } else { BackendError::message(error) });
        }
        let (commands, incoming) = unbounded();
        let (startup_sender, startup_receiver) = oneshot::channel();
        let callbacks = commands.downgrade();
        let remote = Rc::new(WebWorkerClient { commands, next: Cell::new(0) });
        // Transport readiness must not wait behind the backend background-work gate.
        wasm_bindgen_futures::spawn_local(serve_connection(connection, incoming, callbacks, startup_sender));
        // Readiness includes migrations over the user's entire library.
        let startup = startup_receiver.await.map_err(|_| BackendError::from("backend worker stopped during startup"))??;
        Ok((Self { remote }, startup))
    }

    /// `files` are picked browser files sent beside the request under a token.
    fn send_web_request(&self, command: WorkerCommand, events: Option<Sender<WorkerPayload>>, files: Option<(&str, &js_sys::Array)>) -> Result<(u64, oneshot::Receiver<Result<WorkerPayload, BackendError>>), BackendError> {
        let id = self.remote.next.get().checked_add(1).ok_or_else(|| BackendError::message("browser request IDs exhausted"))?;
        self.remote.next.set(id);
        let restore = !command.is_book_subscription();
        let bytes = encode_worker_message(&AppWorkerClientMessage::Request(AppWorkerRequest { id, command }))?;
        let (reply, receiver) = oneshot::channel();
        let files = files.map(|(token, files)| (token.to_owned(), files.clone()));
        self.remote.commands.try_send(Command::Request { id, bytes, reply, events, restore, files }).map_err(|_| BackendError::from("backend worker stopped during request"))?;
        Ok((id, receiver))
    }

    async fn request_value(&self, command: WorkerCommand) -> Result<WorkerPayload, BackendError> {
        let (id, receiver) = self.send_web_request(command, None, None)?;
        let _pending = WebPendingRequest { client: &self.remote, id, subscription: false };
        receiver.await.map_err(|_| "backend worker stopped during request".to_owned())?
    }

    async fn subscribe_value(&self, command: WorkerCommand) -> Result<(u64, WorkerPayload, Receiver<WorkerPayload>), BackendError> {
        let coalesce = matches!(command, WorkerCommand::ManagementSubscription(ManagementSubscription::LibraryList));
        let (event_sender, event_receiver) = if coalesce { bounded(1) } else { unbounded() };
        let (id, response_receiver) = self.send_web_request(command, Some(event_sender), None)?;
        let mut pending = WebPendingRequest { client: &self.remote, id, subscription: true };
        match response_receiver.await.unwrap_or_else(|_| Err("backend worker stopped during subscription".into())) {
            Ok(initial) => {
                pending.subscription = false;
                Ok((id, initial, event_receiver))
            }
            Err(error) => Err(error),
        }
    }

    fn cancel_web_subscription(&self, id: u64) {
        let _ = self.remote.commands.try_send(Command::Cancel { id, subscription: true });
    }
}

impl Transport {
    pub(crate) async fn request<T: WireReply>(&self, command: AppCommand) -> Result<T, BackendError> {
        decode_worker_payload(self.request_value(WorkerCommand::App(command)).await?)
    }

    async fn worker_typed_subscription<Event: serde::de::DeserializeOwned + Send + 'static, Reply: serde::de::DeserializeOwned>(&self, command: WorkerCommand) -> Result<(Receiver<Event>, Reply), BackendError> {
        let (subscription_id, initial, receiver) = self.subscribe_value(command).await?;
        let initial = match decode_worker_payload(initial) {
            Ok(initial) => initial,
            Err(error) => {
                self.cancel_web_subscription(subscription_id);
                return Err(error);
            }
        };
        let (sender, updates) = unbounded();
        let subscription_backend = self.clone();
        spawn_detached(async move {
            while let Some(value) = next_subscription_value(&receiver, &sender).await {
                let Ok(update) = decode_worker_payload(value) else { continue };
                if sender.send(update).await.is_err() {
                    break;
                }
            }
            subscription_backend.cancel_web_subscription(subscription_id);
        });
        Ok((updates, initial))
    }

    async fn unit_subscription(&self, command: WorkerCommand) -> Result<Receiver<()>, BackendError> {
        let (subscription_id, _initial, receiver) = self.subscribe_value(command).await?;
        let (sender, updates) = bounded(1);
        let subscription_backend = self.clone();
        spawn_detached(async move {
            while next_subscription_value(&receiver, &sender).await.is_some() {
                if matches!(sender.try_send(()), Err(async_channel::TrySendError::Closed(()))) {
                    break;
                }
            }
            subscription_backend.cancel_web_subscription(subscription_id);
        });
        Ok(updates)
    }

    async fn changes_transport(&self, command: WorkerCommand) -> Result<watch::Receiver<()>, BackendError> {
        let (subscription_id, _initial, receiver) = self.subscribe_value(command).await?;
        let (sender, updates) = watch::channel(());
        let subscription_backend = self.clone();
        spawn_detached(async move {
            loop {
                tokio::select! {
                    _ = sender.closed() => break,
                    value = receiver.recv() => match value {
                        Ok(_) => { sender.send_replace(()); }
                        Err(_) => break,
                    },
                }
            }
            subscription_backend.cancel_web_subscription(subscription_id);
        });
        Ok(updates)
    }
}

// Dropping a view's receiver cancels its worker subscription even when the
// library is idle and no further event would otherwise wake the bridge.
async fn next_subscription_value<T>(receiver: &Receiver<WorkerPayload>, sender: &Sender<T>) -> Option<WorkerPayload> {
    tokio::select! {
        _ = sender.closed() => None,
        value = receiver.recv() => value.ok(),
    }
}

impl AppClient {
    pub async fn connect_web(config: crate::host::web::WebConfiguration) -> Result<(Self, AppStartupState), BackendError> {
        let (transport, startup) = Transport::connect_web_transport(config).await?;
        let executor = BackendExecutor::new().map_err(BackendError::operation)?;
        let services = Arc::new(Services::new(executor));
        let management = ManagementClient::new(transport.clone());
        let account = AccountClient::new(transport.clone());
        Ok((Self { account, management, transport, services }, startup))
    }
}

impl ManagementTransport for Transport {
    fn library(&self, id: LibraryId) -> LibraryClient {
        LibraryClient::new(id, Connection { transport: self.clone(), id })
    }
    fn request(&self, command: ManagementCommand) -> BoxedBackendFuture<'_, Result<WorkerPayload, BackendError>> {
        Box::pin(self.request_value(WorkerCommand::Management(command)))
    }
    fn list_changes(&self) -> BoxedBackendFuture<'_, Result<watch::Receiver<()>, BackendError>> {
        Box::pin(self.changes_transport(WorkerCommand::ManagementSubscription(ManagementSubscription::LibraryList)))
    }
    fn notification_interest(&self, browsing: bool) -> BoxedBackendFuture<'_, Result<Receiver<()>, BackendError>> {
        Box::pin(self.unit_subscription(WorkerCommand::ManagementSubscription(ManagementSubscription::Notifications { browsing })))
    }
}

struct Connection {
    transport: Transport,
    id: LibraryId,
}
impl ClientTransport for Connection {
    fn request(&self, command: LibraryCommand) -> TransportResult<'_, WorkerPayload> {
        Box::pin(self.transport.request_value(WorkerCommand::Library { library_id: self.id, command }))
    }
    fn updates(&self) -> TransportResult<'_, (library_backend::LibraryUpdates, bool)> {
        Box::pin(async move {
            let (updates, scanning) = self.transport.worker_typed_subscription::<library_backend::LibraryUpdate, bool>(WorkerCommand::LibrarySubscription { library_id: self.id, subscription: LibrarySubscription::Updates }).await?;
            Ok((updates.into(), scanning))
        })
    }
    fn subscribe_book(&self, book_id: BookId) -> TransportResult<'_, (Receiver<()>, ResolvedBookData)> {
        Box::pin(self.transport.worker_typed_subscription(WorkerCommand::LibrarySubscription { library_id: self.id, subscription: LibrarySubscription::Book { book_id } }))
    }
    fn import_directory(&self, parent_id: DirId, directory: DirectoryImport, create_root: bool) -> TransportResult<'_, Vec<ImportFailure>> {
        Box::pin(async move {
            use library_backend::PendingCommand;
            let token = uuid::Uuid::new_v4().to_string();
            let files = js_sys::Array::new();
            let mut paths = Vec::with_capacity(directory.files.len());
            for file in directory.files {
                files.push(&file.source.into_file()?.into());
                paths.push(file.path);
            }
            let command = LibraryCommand::Pending(PendingCommand::ImportSelectedDirectory { parent_id, create_root, name: directory.name, directories: directory.directories, paths, token: token.clone() });
            let (id, receiver) = self.transport.send_web_request(WorkerCommand::Library { library_id: self.id, command }, None, Some((&token, &files)))?;
            let _pending = WebPendingRequest { client: &self.transport.remote, id, subscription: false };
            decode_worker_payload(receiver.await.map_err(|_| "backend worker stopped during import".to_owned())??)
        })
    }
}

impl AccountTransport for Transport {
    fn request(&self, command: AccountCommand) -> BoxedBackendFuture<'_, Result<AccountSnapshot, BackendError>> {
        Box::pin(async move { decode_worker_payload(self.request_value(WorkerCommand::Account(command)).await?) })
    }
}
