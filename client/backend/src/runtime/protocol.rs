use super::*;

pub use library_runtime::wire::{ThumbnailResolution, WorkerPayload, decode_worker_message, decode_worker_payload, encode_reply, encode_worker_message};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct AppStartupState {
    pub libraries: Vec<crate::LibraryEntry>,
    pub browsing_preferences: app_preferences::BrowsingPreferences,
    pub account_status: crate::AccountStatus,
    pub reader_preferences: app_preferences::ReaderPreferences,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum AppWorkerStartupMessage {
    Ready { startup: AppStartupState },
    Failed { error: String },
    Diagnostic { message: String },
    Response { id: u64, result: Result<WorkerPayload, String> },
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

/// A live stream emitted by one already-open library actor.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum LibrarySubscription {
    Updates,
    BookDownload { content_hash: crate::ContentHash },
}

impl From<AppCommand> for WorkerCommand {
    fn from(command: AppCommand) -> Self {
        Self::App(command)
    }
}

impl AppWorkerRequest {
    pub fn is_source_subscription(&self) -> bool {
        matches!(&self.command, WorkerCommand::App(command) if command.is_book_subscription() || matches!(command, AppCommand::SubscribeNotifications { .. }))
    }
    pub fn is_resolve_book(&self) -> bool {
        matches!(&self.command, WorkerCommand::App(command) if command.is_book_subscription())
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum AppWorkerClientMessage {
    Request(AppWorkerRequest),
    CancelSubscription { id: u64 },
}

/// The application-level RPC protocol used by browser pages and their shared
/// backend worker.

/// Operations scoped to one library. Keeping the library identity in the
/// enclosing command prevents paths or database handles crossing the worker.

pub use library_backend::{BookSource, ResolvedBookData};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct AccountSnapshot {
    pub account: crate::AccountStatus,
    pub storage_usage: Option<crate::AccountStorageUsage>,
    pub sync_statuses: Vec<crate::LibrarySyncStatus>,
    pub local_storage_usage: Vec<crate::LocalLibraryStorageUsage>,
    pub server_storage_usage: Option<Vec<crate::ServerLibraryStorageUsage>>,
    pub transfers: Vec<crate::LibraryTransfers>,
}

pub enum WorkerSubscription {
    Book(Box<dyn crate::executor::BackendSend>),
    Unit(async_channel::Receiver<()>),
    LibraryUpdates(async_channel::Receiver<library_model::LibraryUpdate>),
    DownloadStates(async_channel::Receiver<crate::DownloadState>),
}

impl WorkerSubscription {
    pub async fn next_value(&self) -> Option<WorkerPayload> {
        match self {
            Self::Book(_lease) => std::future::pending().await,
            Self::Unit(receiver) => receiver.recv().await.ok().and_then(|()| encode_reply(()).ok()),
            Self::LibraryUpdates(receiver) => receiver.recv().await.ok().and_then(|update| encode_reply(update).ok()),
            Self::DownloadStates(receiver) => receiver.recv().await.ok().and_then(|state| encode_reply(state).ok()),
        }
    }
}

pub enum WorkerDispatchResult<Value = WorkerPayload> {
    Response(Value),
    Subscription { initial: Value, events: WorkerSubscription },
}

#[cfg(test)]
mod wire_tests {
    use super::*;

    #[test]
    fn direct_encoding_preserves_the_named_messagepack_wire_format() {
        for message in [
            AppWorkerClientMessage::Request(AppWorkerRequest { id: 42, command: WorkerCommand::App(AppCommand::CreateLibrary { name: "Böcker".to_owned() }) }),
            AppWorkerClientMessage::CancelSubscription { id: 42 },
        ] {
            let mut previous = b"BKHW\x01".to_vec();
            previous.extend(rmp_serde::to_vec_named(&message).unwrap());
            let encoded = encode_worker_message(&message).unwrap();
            assert_eq!(encoded, previous);
            let restored: AppWorkerClientMessage = decode_worker_message(&encoded).unwrap();
            assert_eq!(encode_worker_message(&restored).unwrap(), encoded);
        }
        assert!(decode_worker_message::<AppWorkerClientMessage>(b"BKHW\x02invalid").is_err());
    }
}
