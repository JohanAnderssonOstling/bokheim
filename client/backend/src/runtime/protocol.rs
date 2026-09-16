use super::*;

const WIRE_HEADER: &[u8; 5] = b"BKHW\x01";

pub fn encode_worker_message<T: serde::Serialize + ?Sized>(message: &T) -> Result<Vec<u8>, String> {
    let mut bytes = WIRE_HEADER.to_vec();
    message.serialize(&mut rmp_serde::Serializer::new(&mut bytes).with_struct_map()).map_err(|error| error.to_string())?;
    Ok(bytes)
}

pub fn decode_worker_message<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    let payload = bytes.strip_prefix(WIRE_HEADER).ok_or_else(|| "unsupported backend worker protocol".to_owned())?;
    rmp_serde::from_slice(payload).map_err(|error| error.to_string())
}

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

/// One encoded reply. Commands that carry bytes serialize them as `serde_bytes`
/// values inside this message rather than as a second payload shape.
#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct WorkerPayload(#[serde(with = "serde_bytes")] pub Vec<u8>);

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
        matches!(&self.command, WorkerCommand::App(command) if command.is_publication_subscription() || matches!(command, AppCommand::SubscribeNotifications { .. }))
    }
    pub fn is_resolve_book(&self) -> bool {
        matches!(&self.command, WorkerCommand::App(command) if command.is_publication_subscription())
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum ThumbnailResolution {
    Browse,
    HighDensity,
}

impl ThumbnailResolution {
    pub const fn width(self) -> u32 {
        match self {
            Self::Browse => thumbnail::BROWSE_THUMBNAIL_WIDTH,
            Self::HighDensity => thumbnail::THUMBNAIL_WIDTH,
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct ResolvedBookData {
    pub pdf_metadata: Option<pdf_view_common::PdfReaderMetadata>,
    pub format: library_model::BookFormat,
    pub source: PublicationSource,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum PublicationSource {
    Local {
        capability: String,
        length: u64,
    },
    Remote {
        source: crate::library::RemoteFile,
        #[serde(with = "serde_bytes")]
        prefix: Vec<u8>,
    },
    Audiobook(crate::library::PlaybackSource),
    /// Native wire adapters can still return owned bytes; browsers use capabilities.
    Inline(#[serde(with = "serde_bytes")] Vec<u8>),
}

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
    Publication(Box<dyn crate::executor::BackendSend>),
    Unit(async_channel::Receiver<()>),
    LibraryUpdates(async_channel::Receiver<library_model::LibraryUpdate>),
    DownloadStates(async_channel::Receiver<crate::DownloadState>),
}

impl WorkerSubscription {
    pub async fn next_value(&self) -> Option<WorkerPayload> {
        match self {
            Self::Publication(_lease) => std::future::pending().await,
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
            let mut previous = WIRE_HEADER.to_vec();
            previous.extend(rmp_serde::to_vec_named(&message).unwrap());
            let encoded = encode_worker_message(&message).unwrap();
            assert_eq!(encoded, previous);
            let restored: AppWorkerClientMessage = decode_worker_message(&encoded).unwrap();
            assert_eq!(encode_worker_message(&restored).unwrap(), encoded);
        }
        assert!(decode_worker_message::<AppWorkerClientMessage>(b"BKHW\x02invalid").is_err());
    }

    #[test]
    fn replies_carrying_bytes_decode_through_the_one_payload_shape() {
        let metadata = encode_reply(7usize).unwrap();
        assert_eq!(decode_worker_payload::<usize>(metadata).unwrap(), 7);
        // Thumbnails and file ranges travel as ordinary `serde_bytes` replies.
        let attachment = encode_reply(Some(serde_bytes::ByteBuf::from(vec![1, 2, 3]))).unwrap();
        assert_eq!(decode_worker_payload::<Option<serde_bytes::ByteBuf>>(attachment).unwrap().map(serde_bytes::ByteBuf::into_vec), Some(vec![1, 2, 3]));
        assert_eq!(decode_worker_payload::<Option<serde_bytes::ByteBuf>>(encode_reply(None::<serde_bytes::ByteBuf>).unwrap()).unwrap(), None);
        assert!(decode_worker_payload::<usize>(WorkerPayload(vec![0])).is_err());
    }
}

pub(crate) fn encode_reply<T: serde::Serialize>(value: T) -> Result<WorkerPayload, String> {
    encode_worker_message(&value).map(WorkerPayload)
}

pub(crate) fn decode_worker_payload<T: serde::de::DeserializeOwned>(payload: WorkerPayload) -> Result<T, String> {
    decode_worker_message(&payload.0)
}
