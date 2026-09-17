//! Worker-only source capabilities and OPFS staging commands.
//! Shared application requests and their reply contracts live in runtime::requests.
use crate::runtime::dispatch::storage_command;
use crate::runtime::*;
use crate::{LibrarySession, app::AppBackend};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum WebLibraryCommand {
    OpenRemoteAudio { content_hash: crate::ContentHash },
    ReadBookRange { capability: String, offset: u64, length: usize },
    ImportStagedBook { parent_id: crate::DirId, file_name: String, physical: String, length: u64, hash: crate::ContentHash },
    CleanupStagedImport { job: String, files: u32 },
}

pub(crate) mod library_requests {
    pub(crate) use super::WebLibraryCommand::*;
}

impl LibraryRequest for WebLibraryCommand {
    fn command(self) -> LibraryCommand {
        self.into()
    }
}
impl From<WebLibraryCommand> for LibraryCommand {
    fn from(command: WebLibraryCommand) -> Self {
        match command {
            WebLibraryCommand::OpenRemoteAudio { content_hash } => Self::OpenRemoteAudio { content_hash },
            WebLibraryCommand::ReadBookRange { capability, offset, length } => Self::ReadBookRange { capability, offset, length },
            WebLibraryCommand::ImportStagedBook { parent_id, file_name, physical, length, hash } => Self::ImportStagedBook { parent_id, file_name, physical, length, hash },
            WebLibraryCommand::CleanupStagedImport { job, files } => Self::CleanupStagedImport { job, files },
        }
    }
}
impl WebLibraryCommand {
    pub(crate) async fn dispatch_library(self, library: &LibrarySession) -> Result<WorkerPayload, String> {
        match self {
            Self::OpenRemoteAudio { content_hash } => {
                let source = crate::library::browser_audiobook::open_remote_audio(library, content_hash).await?;
                encode_reply(source)
            }
            Self::ReadBookRange { capability, offset, length } => {
                // The storage command outlives this borrow, so it carries the
                // library identity by value rather than the session reference.
                let library_id = *library.id();
                let bytes = storage_command(move || Ok(serde_bytes::ByteBuf::from(crate::library::browser_book::read_range(library_id, &capability, offset, length)?))).await?;
                encode_reply(bytes)
            }
            Self::ImportStagedBook { parent_id, file_name, physical, length, hash } => {
                let hash = library.import_staged_book(parent_id, file_name, physical, length, hash).await?;
                encode_reply(hash)
            }
            Self::CleanupStagedImport { job, files } => {
                storage_command(move || crate::platform::web_storage::cleanup_staged_import(&job, files)).await?;
                encode_reply(())
            }
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum WebAppCommand {
    SubscribeBook { library_id: sync_common::LibraryId, content_hash: crate::ContentHash },
}
impl WebAppCommand {
    pub(crate) fn is_book_subscription(&self) -> bool {
        matches!(self, Self::SubscribeBook { .. })
    }
    /// The subscription carries no events: its receiver is the reader's lease,
    /// and dropping it releases the worker-owned book generation.
    pub(crate) async fn dispatch_app(self, backend: AppBackend) -> Result<WorkerDispatchResult, String> {
        match self {
            Self::SubscribeBook { library_id, content_hash } => {
                let library = backend.library_directory().get(&library_id).map_err(|error| error.to_string())?;
                let (initial, lease) = library.subscribe_book(content_hash).await?;
                Ok(WorkerDispatchResult::Subscription { initial, events: WorkerSubscription::Book(Box::new(lease)) })
            }
        }
    }
}

impl From<WebAppCommand> for AppCommand {
    fn from(command: WebAppCommand) -> Self {
        Self::Host(command)
    }
}

#[cfg(feature = "web-runtime-tests")]
pub(crate) fn wire_contract() {
    // Extensions remain flat on the existing wire, and keep their typed shape
    // after MessagePack decoding. No target-specific wrapper leaks to callers.
    let command = library_requests::ImportStagedBook { parent_id: crate::ROOT_DIR_ID, file_name: "Book.epub".into(), physical: "staged/book".into(), length: 12, hash: "a".repeat(64).parse().unwrap() }.command();
    let json = serde_json::to_value(&command).unwrap();
    assert!(json.get("ImportStagedBook").is_some() && json.get("Host").is_none());
    let restored: LibraryCommand = decode_worker_message(&encode_worker_message(&command).unwrap()).unwrap();
    assert!(matches!(restored, LibraryCommand::ImportStagedBook { length: 12, .. }));
    let command = AppCommand::from(WebAppCommand::SubscribeBook { library_id: crate::LibraryId::from_u128(1), content_hash: "a".repeat(64).parse().unwrap() });
    let json = serde_json::to_value(&command).unwrap();
    assert!(json.get("SubscribeBook").is_some() && json.get("Host").is_none());
    let restored: AppCommand = decode_worker_message(&encode_worker_message(&command).unwrap()).unwrap();
    assert!(restored.is_book_subscription());
}
