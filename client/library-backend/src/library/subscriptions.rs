//! Subscription requests and live resources owned by one library.

use crate::library::{LibraryCommand, ReadyCommand, SessionCommand};

use crate::ContentHash;
use client_runtime::reply::Reply;
use client_runtime::wire::{encode_reply, WorkerPayload};

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum LibrarySubscription {
    Updates,
    BookDownload {
        content_hash: ContentHash,
    },
    #[cfg(target_arch = "wasm32")]
    Book {
        content_hash: ContentHash,
    },
}
impl LibrarySubscription {}

pub enum LibraryEvents {
    #[cfg(target_arch = "wasm32")]
    Book(Box<dyn crate::executor::BackendSend>),
    Updates(async_channel::Receiver<library_model::LibraryUpdate>),
    DownloadStates(async_channel::Receiver<crate::DownloadState>),
}
impl LibraryEvents {
    pub async fn next_value(&self) -> Option<WorkerPayload> {
        match self {
            #[cfg(target_arch = "wasm32")]
            Self::Book(_lease) => std::future::pending().await,
            Self::Updates(receiver) => receiver.recv().await.ok().and_then(|update| encode_reply(update).ok()),
            Self::DownloadStates(receiver) => receiver.recv().await.ok().and_then(|state| encode_reply(state).ok()),
        }
    }
}

impl super::LibraryHandle {
    pub async fn subscribe(&self, subscription: LibrarySubscription) -> Result<(Reply, LibraryEvents), crate::BackendError> {
        match subscription {
            LibrarySubscription::Updates => {
                let events = self.subscribe_updates();
                let scanning = self.dispatch(LibraryCommand::Session(SessionCommand::Ready(ReadyCommand::Scanning))).await?.take::<bool>()?;
                Ok((Reply::new(scanning), LibraryEvents::Updates(events)))
            }
            LibrarySubscription::BookDownload { content_hash } => {
                let (events, initial) = self.download_changes(content_hash).await?;
                Ok((Reply::new(initial), LibraryEvents::DownloadStates(events)))
            }
            #[cfg(target_arch = "wasm32")]
            LibrarySubscription::Book { content_hash } => {
                let (initial, lease) = self.subscribe_book(content_hash).await?;
                Ok((initial, LibraryEvents::Book(Box::new(lease))))
            }
        }
    }
}

#[cfg(test)]
mod reconnect_tests {
    use super::*;
    use client_runtime::wire::decode_worker_payload;
}
