use super::*;
use crate::{AccountStatus, BackendError};
use library_backend::{LibraryCommand, LibrarySyncStatus};
use tokio::sync::watch;

#[cfg(target_arch = "wasm32")]
pub use client_runtime::wire::decode_worker_payload;
pub use client_runtime::wire::{WorkerPayload, decode_worker_message, encode_reply, encode_worker_message};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct AppStartupState {
    pub libraries: Vec<crate::LibraryEntry>,
    pub browsing_preferences: app_preferences::BrowsingPreferences,
    pub account_status: AccountStatus,
    pub reader_preferences: app_preferences::ReaderPreferences,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum AppWorkerStartupMessage {
    Ready { startup: AppStartupState },
    Failed { error: BackendError },
    Response { id: u64, result: Result<WorkerPayload, BackendError> },
    Event { id: u64, payload: WorkerPayload },
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct AppWorkerRequest {
    pub id: u64,
    pub command: WorkerCommand,
}

/// Top-level worker routing. Library requests bypass the application-command
/// dispatcher and target one already-open library actor directly.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum WorkerCommand {
    App(AppCommand),
    Library { library_id: LibraryId, command: LibraryCommand },
    LibrarySubscription { library_id: LibraryId, subscription: LibrarySubscription },
}

pub use library_backend::library::subscriptions::{LibraryEvents, LibrarySubscription};

impl WorkerCommand {
    pub(crate) fn is_book_subscription(&self) -> bool {
        match self {
            Self::LibrarySubscription { subscription, .. } => {
                #[cfg(target_arch = "wasm32")]
                {
                    matches!(subscription, LibrarySubscription::Book { .. })
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let _ = subscription;
                    false
                }
            }
            Self::App(_) | Self::Library { .. } => false,
        }
    }
}

impl From<AppCommand> for WorkerCommand {
    fn from(command: AppCommand) -> Self {
        Self::App(command)
    }
}

impl AppWorkerRequest {
    pub fn is_source_subscription(&self) -> bool {
        self.command.is_book_subscription() || matches!(&self.command, WorkerCommand::App(AppCommand::SubscribeNotifications { .. }))
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum AppWorkerClientMessage {
    Request(AppWorkerRequest),
    CancelSubscription { id: u64 },
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct AccountSnapshot {
    pub account: AccountStatus,
    pub storage_usage: Option<account_contract::StorageUsageResponse>,
    pub sync_statuses: Vec<LibrarySyncStatus>,
    pub server_storage_usage: Option<Vec<crate::ServerLibraryStorageUsage>>,
}

pub enum WorkerSubscription {
    Library(LibraryEvents),
    LibraryList(watch::Receiver<()>),
    Unit(async_channel::Receiver<()>),
}

impl WorkerSubscription {
    pub async fn next_value(&mut self) -> Option<WorkerPayload> {
        match self {
            Self::Library(events) => events.next_value().await,
            Self::LibraryList(receiver) => receiver.changed().await.ok().and_then(|()| encode_reply(()).ok()),
            Self::Unit(receiver) => receiver.recv().await.ok().and_then(|()| encode_reply(()).ok()),
        }
    }
}

pub enum WorkerDispatchResult<Value = WorkerPayload> {
    Response(Value),
    Subscription { initial: Value, events: WorkerSubscription },
}

#[cfg(target_arch = "wasm32")]
impl WorkerDispatchResult<client_runtime::reply::Reply> {
    pub(crate) fn into_wire(self) -> Result<WorkerDispatchResult, BackendError> {
        Ok(match self {
            Self::Response(value) => WorkerDispatchResult::Response(value.into_wire()?),
            Self::Subscription { initial, events } => WorkerDispatchResult::Subscription { initial: initial.into_wire()?, events },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_backend::BookId;

    #[tokio::test]
    async fn library_list_watch_delivers_coalesced_wire_notifications_and_closes() {
        let (sender, receiver) = watch::channel(());
        let mut events = WorkerSubscription::LibraryList(receiver);
        sender.send_replace(());
        sender.send_replace(());
        let payload = events.next_value().await.unwrap();
        client_runtime::wire::decode_worker_payload::<()>(payload).unwrap();
        drop(sender);
        assert!(events.next_value().await.is_none(), "coalesced notifications should be consumed before closure");
    }

    #[test]
    fn rejects_flat_book_subscription_requests() {
        let request = serde_json::json!({
            "Request": {
                "id": 1,
                "command": {
                    "App": {
                        "SubscribeBook": {
                            "library_id": LibraryId::new_v4(),
                            "content_hash": BookId::from_bytes([7; 32]),
                        }
                    }
                }
            }
        });
        let bytes = encode_worker_message(&request).unwrap();
        let error = decode_worker_message::<AppWorkerClientMessage>(&bytes).unwrap_err();
        assert!(error.to_string().contains("unknown variant `SubscribeBook`"), "{error}");
    }

    #[test]
    fn accepts_library_scoped_subscriptions() {
        let library_id = LibraryId::new_v4();
        let request = AppWorkerClientMessage::Request(AppWorkerRequest { id: 1, command: WorkerCommand::LibrarySubscription { library_id, subscription: LibrarySubscription::Updates } });
        let bytes = encode_worker_message(&request).unwrap();
        let decoded = decode_worker_message::<AppWorkerClientMessage>(&bytes).unwrap();
        assert!(matches!(decoded, AppWorkerClientMessage::Request(AppWorkerRequest { id: 1, command: WorkerCommand::LibrarySubscription { library_id: id, subscription: LibrarySubscription::Updates } }) if id == library_id));
    }
}
