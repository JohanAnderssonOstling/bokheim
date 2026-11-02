use super::*;
use crate::{AccountStatus, BackendError};
use library_backend::management::{ManagementCommand, ManagementSubscription};
use library_backend::LibraryCommand;
use tokio::sync::watch;

#[cfg(target_arch = "wasm32")]
pub use client_runtime::wire::decode_worker_payload;
pub use client_runtime::wire::{decode_worker_message, encode_reply, encode_worker_message, WorkerPayload};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct AppStartupState {
    pub default_library_root: String,
    pub dictionary_storage_root: String,
    pub preference_storage_root: String,
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
    Account(library_backend::account::Command),
    Management(ManagementCommand),
    ManagementSubscription(ManagementSubscription),
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
            Self::App(_) | Self::Account(_) | Self::Management(_) | Self::ManagementSubscription(_) | Self::Library { .. } => false,
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
        self.command.is_book_subscription() || matches!(&self.command, WorkerCommand::ManagementSubscription(ManagementSubscription::Notifications { .. }))
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum AppWorkerClientMessage {
    Request(AppWorkerRequest),
    CancelSubscription { id: u64 },
}

pub use library_backend::account::AccountSnapshot;

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
#[path = "../../tests/runtime/protocol/tests.rs"]
mod tests;
