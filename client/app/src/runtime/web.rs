use crate::BackendError;
use crate::executor::spawn_detached;
use crate::web::WebConnection;
use library_backend::{ContentHash, DirId, DirectoryImport, ImportFailure, LibraryCommand, LibraryId, ResolvedBookData};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use tokio::sync::watch;
#[derive(Clone)]
pub(crate) struct Transport {
    remote: Rc<WebWorkerClient>,
}

use super::*;
use client_platform_runtime::pending::Pending;

use client_platform_web::transport::session::{Session, SessionMessage, SessionSender};

struct WebPendingRequest<'a> {
    client: &'a WebWorkerClient,
    id: u64,
    subscription: bool,
}

impl Drop for WebPendingRequest<'_> {
    fn drop(&mut self) {
        self.client.pending.take(self.id);
        if self.subscription {
            self.client.subscriptions.borrow_mut().remove(&self.id);
            self.client.subscription_commands.borrow_mut().remove(&self.id);
            if let Ok(bytes) = encode_worker_message(&AppWorkerClientMessage::CancelSubscription { id: self.id }) {
                let _ = self.client.port.send(&bytes);
            }
        }
    }
}

pub(crate) struct WebWorkerClient {
    _session: Session,
    _connection: WebConnection,
    port: SessionSender,
    failure: Rc<RefCell<Option<BackendError>>>,
    recovering: Rc<Cell<bool>>,
    subscription_commands: Rc<RefCell<HashMap<u64, Vec<u8>>>>,
    pending: Rc<Pending<oneshot::Sender<Result<WorkerPayload, BackendError>>>>,
    subscriptions: Rc<RefCell<HashMap<u64, async_channel::Sender<WorkerPayload>>>>,
}

impl Transport {
    pub(crate) async fn connect_web_transport(config: crate::web::WebConfiguration) -> Result<(Self, AppStartupState), BackendError> {
        let connection = WebConnection::connect(config).await?;
        Self::finish_web_connection(connection).await
    }

    async fn finish_web_connection(connection: WebConnection) -> Result<(Self, AppStartupState), BackendError> {
        let port = SessionSender::new(connection.worker.clone(), connection.port());

        let pending = Rc::new(Pending::<oneshot::Sender<Result<WorkerPayload, BackendError>>>::default());
        let (startup_sender, startup_receiver) = oneshot::channel();
        let startup_sender = Rc::new(RefCell::new(Some(startup_sender)));
        let failure = Rc::new(RefCell::new(None::<BackendError>));
        let recovering = Rc::new(Cell::new(false));
        let subscription_commands = Rc::new(RefCell::new(HashMap::<u64, Vec<u8>>::new()));
        let response_pending = pending.clone();
        let subscriptions = Rc::new(RefCell::new(HashMap::<u64, async_channel::Sender<WorkerPayload>>::new()));
        let response_subscriptions = subscriptions.clone();
        let fail: Rc<dyn Fn(BackendError)> = {
            let failure = failure.clone();
            let startup = startup_sender.clone();
            let pending = pending.clone();
            let subscriptions = subscriptions.clone();
            let port = port.clone();
            Rc::new(move |error| {
                if failure.borrow().is_some() {
                    return;
                }
                *failure.borrow_mut() = Some(error.clone());
                if let Some(sender) = startup.borrow_mut().take() {
                    let _ = sender.send(Err(error.clone()));
                }
                pending.fail_all(|sender| {
                    let _ = sender.send(Err(error.clone()));
                });
                subscriptions.borrow_mut().clear();
                port.close();
            })
        };
        let message_failure = fail.clone();
        let message_recovering = recovering.clone();
        let message_commands = subscription_commands.clone();
        let message_port = port.clone();
        let on_message = move |message: SessionMessage| {
            if matches!(&message, SessionMessage::Reconnecting) {
                message_recovering.set(true);
                // A capability pins one worker-owned generation; reopening it
                // silently would mix new bytes with the reader's old cache.
                response_subscriptions.borrow_mut().retain(|id, _| message_commands.borrow().contains_key(id));
                response_pending.fail_all(|sender| {
                    let _ = sender.send(Err("Database connection interrupted. A write may already have committed; check its result before retrying.".into()));
                });
                return;
            }
            let parsed = match message {
                SessionMessage::Bytes(bytes) => decode_worker_message::<AppWorkerStartupMessage>(&bytes),
                SessionMessage::Failed(error) => Ok(AppWorkerStartupMessage::Failed { error: error.into() }),
                SessionMessage::Reconnecting => unreachable!(),
            };
            match parsed {
                Ok(AppWorkerStartupMessage::Ready { startup }) => {
                    if message_recovering.replace(false) {
                        for bytes in message_commands.borrow().values() {
                            if let Err(error) = message_port.send(bytes) {
                                message_failure(BackendError::operation(error).context("Could not restore database subscription"));
                                return;
                            }
                        }
                    }
                    if let Some(sender) = startup_sender.borrow_mut().take() {
                        let _ = sender.send(Ok(startup));
                    }
                }
                Ok(AppWorkerStartupMessage::Failed { error }) => message_failure(error),
                Ok(AppWorkerStartupMessage::Response { id, result }) => {
                    let pending = response_pending.take(id);
                    if let Some(sender) = pending {
                        let _ = sender.send(result);
                    } else {
                        let sender = response_subscriptions.borrow().get(&id).cloned();
                        let command = message_commands.borrow().get(&id).cloned();
                        if let (Some(sender), Some(command)) = (sender, command) {
                            // Reconnecting subscriptions must refresh views that
                            // may have missed events while the host was unavailable.
                            match restored_subscription_updates(&command, result) {
                                Ok(updates) => {
                                    for update in updates {
                                        let _ = sender.try_send(update);
                                    }
                                }
                                Err(error) => message_failure(BackendError::operation(error).context("Could not restore database subscription")),
                            }
                        }
                    }
                }
                Ok(AppWorkerStartupMessage::Event { id, payload }) => {
                    if let Some(sender) = response_subscriptions.borrow().get(&id) {
                        let _ = sender.try_send(payload);
                    }
                }
                Err(error) => message_failure(error.context("invalid backend response")),
            }
        };
        let session = Session::new(port.clone(), on_message, move |error| fail(error.into()));
        // Readiness includes migrations over the user's entire library. Wait
        // for their result; transport/worker failures still resolve this channel.
        let startup = match startup_receiver.await {
            Ok(Ok(startup)) => startup,
            Ok(Err(error)) => {
                return Err(error);
            }
            Err(_) => {
                return Err("backend worker stopped during startup".into());
            }
        };
        Ok((Self { remote: Rc::new(WebWorkerClient { _connection: connection, port, failure, recovering, subscription_commands, pending, subscriptions, _session: session }) }, startup))
    }

    /// `files` are picked browser files sent beside the request under a token.
    fn send_web_request(&self, command: WorkerCommand, events: Option<async_channel::Sender<WorkerPayload>>, files: Option<(&str, &js_sys::Array)>) -> Result<(u64, oneshot::Receiver<Result<WorkerPayload, BackendError>>), BackendError> {
        if let Some(error) = self.remote.failure.borrow().as_ref() {
            return Err(error.clone());
        }
        if self.remote.recovering.get() {
            return Err("Database connection is being restored. Try again shortly.".into());
        }
        let id = self.remote.pending.next_id();
        let restore = !command.is_book_subscription();
        let bytes = encode_worker_message(&AppWorkerClientMessage::Request(AppWorkerRequest { id, command }))?;
        let (sender, receiver) = oneshot::channel();
        self.remote.pending.insert(id, sender);
        if let Some(events) = events {
            if restore {
                self.remote.subscription_commands.borrow_mut().insert(id, bytes.clone());
            }
            self.remote.subscriptions.borrow_mut().insert(id, events);
        }
        let sent = match files {
            Some((token, files)) => self.remote.port.send_with_files(&bytes, token, files),
            None => self.remote.port.send(&bytes),
        };
        if let Err(error) = sent {
            self.remote.pending.take(id);
            self.remote.subscriptions.borrow_mut().remove(&id);
            self.remote.subscription_commands.borrow_mut().remove(&id);
            return Err(BackendError::operation(error).context("failed to send backend request"));
        }
        Ok((id, receiver))
    }

    pub(crate) async fn request_value(&self, command: AppCommand) -> Result<WorkerPayload, BackendError> {
        let (id, receiver) = self.send_web_request(WorkerCommand::App(command), None, None)?;
        let _pending = WebPendingRequest { client: &self.remote, id, subscription: false };
        receiver.await.map_err(|_| "backend worker stopped during request".to_owned())?
    }

    async fn subscribe_value(&self, command: WorkerCommand) -> Result<(u64, WorkerPayload, async_channel::Receiver<WorkerPayload>), BackendError> {
        let coalesce = matches!(command, WorkerCommand::App(AppCommand::SubscribeLibraryList));
        let (event_sender, event_receiver) = if coalesce { async_channel::bounded(1) } else { async_channel::unbounded() };
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
        self.remote.subscriptions.borrow_mut().remove(&id);
        self.remote.subscription_commands.borrow_mut().remove(&id);
        if let Ok(bytes) = encode_worker_message(&AppWorkerClientMessage::CancelSubscription { id }) {
            let _ = self.remote.port.send(&bytes);
        }
    }
}

impl Transport {
    pub(crate) async fn request<T: LibraryReply>(&self, command: AppCommand) -> Result<T, BackendError> {
        decode_worker_payload(self.request_value(command).await?)
    }

    pub(crate) async fn library_updates_transport(&self, library_id: LibraryId) -> Result<(async_channel::Receiver<library_backend::LibraryUpdate>, bool), BackendError> {
        self.worker_typed_subscription(WorkerCommand::LibrarySubscription { library_id, subscription: LibrarySubscription::Updates }).await
    }

    pub(crate) async fn book_subscription(&self, library_id: LibraryId, content_hash: ContentHash) -> Result<(async_channel::Receiver<()>, ResolvedBookData), BackendError> {
        self.worker_typed_subscription(WorkerCommand::LibrarySubscription { library_id, subscription: LibrarySubscription::Book { content_hash } }).await
    }

    async fn worker_typed_subscription<Event: serde::de::DeserializeOwned + Send + 'static, Reply: serde::de::DeserializeOwned>(&self, command: WorkerCommand) -> Result<(async_channel::Receiver<Event>, Reply), BackendError> {
        let (subscription_id, initial, receiver) = self.subscribe_value(command).await?;
        let initial = match decode_worker_payload(initial) {
            Ok(initial) => initial,
            Err(error) => {
                self.cancel_web_subscription(subscription_id);
                return Err(error);
            }
        };
        let (sender, updates) = async_channel::unbounded();
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

    pub(super) async fn unit_subscription(&self, command: AppCommand) -> Result<async_channel::Receiver<()>, BackendError> {
        let (subscription_id, _initial, receiver) = self.subscribe_value(WorkerCommand::App(command)).await?;
        let (sender, updates) = async_channel::bounded(1);
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

    pub(crate) async fn library_list_changes_transport(&self) -> Result<watch::Receiver<()>, BackendError> {
        let (subscription_id, _initial, receiver) = self.subscribe_value(WorkerCommand::App(AppCommand::SubscribeLibraryList)).await?;
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
async fn next_subscription_value<T>(receiver: &async_channel::Receiver<WorkerPayload>, sender: &async_channel::Sender<T>) -> Option<WorkerPayload> {
    tokio::select! {
        _ = sender.closed() => None,
        value = receiver.recv() => value.ok(),
    }
}

fn restored_subscription_updates(command: &[u8], result: Result<WorkerPayload, BackendError>) -> Result<Vec<WorkerPayload>, BackendError> {
    let initial = result?;
    let request: AppWorkerClientMessage = decode_worker_message(command)?;
    if let AppWorkerClientMessage::Request(AppWorkerRequest { command: WorkerCommand::LibrarySubscription { subscription, .. }, .. }) = request {
        match subscription {
            LibrarySubscription::Updates => {
                let scanning = client_runtime::wire::decode_worker_payload::<bool>(initial)?;
                [library_model::LibraryUpdate::Contents, library_model::LibraryUpdate::Scanning(scanning)].into_iter().map(client_runtime::wire::encode_reply).collect()
            }
            _ => Ok(vec![initial]),
        }
    } else {
        // Application subscriptions use their initial reply to signal a refresh.
        Ok(vec![initial])
    }
}

impl Transport {
    pub(crate) async fn library_request(&self, library_id: LibraryId, command: LibraryCommand) -> Result<WorkerPayload, BackendError> {
        let command = WorkerCommand::Library { library_id, command: command };
        let (id, receiver) = self.send_web_request(command, None, None)?;
        let _pending = WebPendingRequest { client: &self.remote, id, subscription: false };
        // The outer result reports a lost worker; the inner one is the reply the
        // library actor produced.
        receiver.await.map_err(|_| "backend worker stopped during request".to_owned())?
    }
}

impl AppClient {
    pub async fn connect_web(config: crate::host::web::WebConfiguration) -> Result<(Self, AppStartupState), BackendError> {
        let (transport, startup) = Transport::connect_web_transport(config).await?;
        Ok((Self { transport }, startup))
    }
}

impl Transport {
    /// Hands a picked folder to the library: its structure in the command,
    /// its files beside it. The import runs in the database worker.
    pub(crate) async fn import_selected_directory(&self, library_id: LibraryId, parent_id: DirId, directory: DirectoryImport, create_root: bool) -> Result<Vec<ImportFailure>, BackendError> {
        use library_backend::PendingCommand;
        let token = uuid::Uuid::new_v4().to_string();
        let files = js_sys::Array::new();
        let mut paths = Vec::with_capacity(directory.files.len());
        for file in directory.files {
            files.push(&file.source.into_file()?.into());
            paths.push(file.path);
        }
        let command = LibraryCommand::Pending(PendingCommand::ImportSelectedDirectory { parent_id, create_root, name: directory.name, directories: directory.directories, paths, token: token.clone() });
        let (id, receiver) = self.send_web_request(WorkerCommand::Library { library_id, command }, None, Some((&token, &files)))?;
        let _pending = WebPendingRequest { client: &self.remote, id, subscription: false };
        decode_worker_payload(receiver.await.map_err(|_| "backend worker stopped during import".to_owned())??)
    }
}
