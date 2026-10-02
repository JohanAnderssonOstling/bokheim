//! Execution of a library protocol command by its owning session.
//!
//! The runtime defines the wire command, but only the library module is
//! allowed to interpret it against a resource-bearing LibrarySession.

use crate::executor::BackendOutput;
use crate::library::RemoteFile;
pub(crate) use crate::ThumbnailResolution;
use crate::{BackendError, ContentHash, DirId};
pub(crate) use client_runtime::reply::Reply;
use std::path::PathBuf;

pub(crate) use super::pending::OperationKind;
pub(crate) use super::LibrarySession;
pub(crate) use futures_util::future::LocalBoxFuture;
pub(crate) use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
pub(crate) mod web;

pub(crate) enum PreparedRequest {
    Ready(Reply),
    Pending(PendingOperation),
}

/// Owned asynchronous work and the scheduling policy required to run it.
pub(crate) struct PendingOperation {
    pub(crate) kind: OperationKind,
    pub(crate) future: LocalBoxFuture<'static, Result<Reply, BackendError>>,
}
impl PendingOperation {
    fn new(kind: OperationKind, future: LocalBoxFuture<'static, Result<Reply, BackendError>>) -> Self {
        Self { kind, future }
    }
}

/// Preparation owns the activity until the durable import coordinator takes
/// over. Dropping an unpolled/rejected operation or unwinding cannot leak it.
pub(crate) struct PreparingImport {
    pub(crate) library: Rc<LibrarySession>,
    pub(crate) activity: uuid::Uuid,
    pub(crate) handed_off: bool,
}
impl PreparingImport {
    fn hand_off(mut self) {
        self.handed_off = true;
    }
}
impl Drop for PreparingImport {
    fn drop(&mut self) {
        if !self.handed_off {
            self.library.finish_directory_import(self.activity);
        }
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum LibraryCommand {
    Runtime(RuntimeCommand),
    Session(SessionCommand),
}

/// Commands that use only shared coordination state and run on the caller.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum RuntimeCommand {
    ImportProgress { activity: uuid::Uuid },
    FinishDirectoryImport { activity: uuid::Uuid },
    DirectoryImportProgress,
    AdvanceDirectoryImport { activity: uuid::Uuid, total: u64, succeeded: u64, failed: u64 },
}

/// Actor-owned commands. Shared runtime commands cannot enter the actor inbox.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum SessionCommand {
    Ready(ReadyCommand),
    Pending(PendingCommand),
}

/// Commands that produce their reply synchronously in the owning actor.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ReadyCommand {
    Scanning,
    DownloadState {
        content_hash: ContentHash,
    },
    TransferSnapshot,
    SyncStatus {
        signed_in: bool,
        remote_present: Option<bool>,
    },
    BookCount,
    Home {
        limit: i32,
    },
    DownloadBook {
        content_hash: ContentHash,
    },
    DownloadBrowse {
        query: library_model::LibraryBrowseQuery,
    },
    RemoveLocalBookCopy {
        content_hash: ContentHash,
    },
    EvictFolderDownloads {
        directory_id: DirId,
    },
    FolderDestinations,
    Trash,
    RestoreBook {
        content_hash: ContentHash,
    },
    RestoreDirectory {
        directory_id: DirId,
        parent_id: Option<DirId>,
    },
    PurgeBook {
        content_hash: ContentHash,
    },
    PurgeDirectory {
        directory_id: DirId,
    },
    EmptyTrash,
    CreateDirectory {
        parent_id: DirId,
        name: String,
    },
    RenameDirectory {
        directory_id: DirId,
        name: String,
    },
    MoveDirectory {
        directory_id: DirId,
        parent_id: DirId,
    },
    CopyDirectory {
        directory_id: DirId,
        parent_id: DirId,
    },
    RestoreBookPlacement {
        content_hash: ContentHash,
        directory_id: DirId,
    },
    CopyBookToDirectory {
        content_hash: ContentHash,
        source_id: DirId,
        destination_id: DirId,
    },
    MoveBookToDirectory {
        content_hash: ContentHash,
        source_id: DirId,
        destination_id: DirId,
    },
    RemoveBookFromDirectory {
        content_hash: ContentHash,
        source_id: DirId,
    },
    MoveBookToTrash {
        content_hash: ContentHash,
    },
    MoveDirectoryToTrash {
        directory_id: DirId,
    },
    Browse {
        query: library_model::LibraryBrowseQuery,
    },
    BookDetail {
        content_hash: ContentHash,
    },
    Authors {
        sort: library_model::AuthorSort,
    },
    BookFormat {
        content_hash: ContentHash,
    },
    ReadingPosition {
        content_hash: ContentHash,
    },
    PdfReadingPosition {
        content_hash: ContentHash,
    },
    UpdateReadingPosition {
        content_hash: ContentHash,
        position: String,
        progress: Option<f32>,
        entry: Option<String>,
    },
    Annotations {
        content_hash: ContentHash,
    },
    UpsertAnnotation {
        annotation: book_model::ReaderAnnotation,
    },
    DeleteAnnotation {
        annotation_id: String,
        modified_at: i64,
    },
    #[cfg(target_arch = "wasm32")]
    ReadBookRange {
        capability: String,
        offset: u64,
        length: usize,
    },
    CleanupImport {
        job: String,
        files: u32,
    },
}

/// Storage adapters resolve stored sources; callers use the same import command.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ImportBookSource {
    Bytes(#[serde(with = "serde_bytes")] Vec<u8>),
    Stored { location: PathBuf, staged: Option<StagedImportMetadata> },
}

/// Metadata already computed by the durable staging coordinator.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StagedImportMetadata {
    pub length: u64,
    pub hash: ContentHash,
}

/// Commands that prepare owned asynchronous work for the actor scheduler.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum PendingCommand {
    LocalStorageUsage,
    // Renewal only authorizes the original remote book. It must not reopen
    // metadata or change source type if the user downloaded a local copy.
    RenewRemoteAudio {
        content_hash: ContentHash,
    },
    ReadFileRange {
        source: RemoteFile,
        offset: u64,
        length: usize,
    },
    ReadFileBundle {
        source: RemoteFile,
        ranges: Vec<(u64, usize)>,
    },
    Thumbnail {
        content_hash: ContentHash,
        resolution: ThumbnailResolution,
    },
    ImportBook {
        parent_id: DirId,
        file_name: String,
        source: ImportBookSource,
    },
    PrepareStagedDirectoryImport {
        activity: uuid::Uuid,
        job: String,
        parent_id: DirId,
        name: Option<String>,
        directories: Vec<Vec<String>>,
    },
    AudiobookPlaybackMetadata {
        content_hash: ContentHash,
    },
    #[cfg(target_arch = "wasm32")]
    OpenRemoteAudio {
        content_hash: ContentHash,
    },
}

/// A serialized reply produced by a library command.
pub trait LibraryReply: BackendOutput + serde::Serialize + serde::de::DeserializeOwned {}
impl<T: BackendOutput + serde::Serialize + serde::de::DeserializeOwned> LibraryReply for T {}

/// Commands that need only shared coordination state can run on the caller.
pub(crate) fn dispatch_runtime_request(runtime: &super::session::LibraryRuntime, events: &super::events::LibraryEventSender, sync_wake: &library_runtime::scheduler::SyncScheduler, command: RuntimeCommand) -> Reply {
    use super::events::LibraryEvent;
    match command {
        RuntimeCommand::ImportProgress { activity } => Reply::new(runtime.directory_import_details_for(Some(activity))),
        RuntimeCommand::DirectoryImportProgress => Reply::new(runtime.directory_import_details()),
        RuntimeCommand::AdvanceDirectoryImport { activity, total, succeeded, failed } => {
            if runtime.advance_directory_import(activity, total, succeeded, failed) {
                let _ = events.try_send(LibraryEvent::TransferStatusChanged);
            }
            Reply::new(())
        }
        RuntimeCommand::FinishDirectoryImport { activity } => {
            if runtime.finish_directory_import(activity) {
                let _ = events.try_send(LibraryEvent::ImportComplete);
                sync_wake.wake();
            }
            Reply::new(())
        }
    }
}

/// Executes short commands in receive order, or prepares owned async work.
/// The exhaustive match is the ordering audit: adding an async operation must
/// choose its concurrency/cancellation class explicitly. Native replies stay
/// typed; encoding still belongs only to the worker boundary.
pub(crate) fn prepare_library_request(library: Rc<LibrarySession>, command: SessionCommand) -> Result<PreparedRequest, BackendError> {
    match command {
        SessionCommand::Ready(command) => Ok(PreparedRequest::Ready(execute_ready(&library, command)?)),
        SessionCommand::Pending(command) => Ok(PreparedRequest::Pending(prepare_pending(library, command))),
    }
}

/// Executes synchronous commands in receive order.
fn execute_ready(library: &LibrarySession, command: ReadyCommand) -> Result<Reply, BackendError> {
    Ok(match command {
        ReadyCommand::Scanning => Reply::new(library.scanner_running()),
        ReadyCommand::DownloadState { content_hash } => Reply::new(library.download_state(content_hash)?),
        ReadyCommand::TransferSnapshot => Reply::new(library.transfer_snapshot()?),
        ReadyCommand::SyncStatus { signed_in, remote_present } => Reply::new(library.sync_status(signed_in, remote_present)?),
        ReadyCommand::BookCount => Reply::new(library.book_count()?),
        ReadyCommand::Home { limit } => Reply::new(library.db.browse_home(limit, library.downloaded_books_only()?).map_err(BackendError::operation)?),
        ReadyCommand::DownloadBook { content_hash } => Reply::new(library.queue_download(content_hash)?),
        ReadyCommand::DownloadBrowse { query } => Reply::new(library.queue_browse_downloads(query)?),
        ReadyCommand::RemoveLocalBookCopy { content_hash } => Reply::new(library.remove_local_book_copy(content_hash)?),
        ReadyCommand::EvictFolderDownloads { directory_id } => Reply::new(library.evict_folder_downloads(&directory_id)?),
        ReadyCommand::FolderDestinations => Reply::new(library.db.folder_destinations()?),
        ReadyCommand::Trash => Reply::new(library.db.library_trash()?),
        ReadyCommand::RestoreBook { content_hash } => Reply::new(library.restore_book(&content_hash)?),
        ReadyCommand::RestoreDirectory { directory_id, parent_id } => Reply::new(library.restore_directory(&directory_id, parent_id.as_ref())?),
        ReadyCommand::PurgeBook { content_hash } => Reply::new(library.purge_book(&content_hash)?),
        ReadyCommand::PurgeDirectory { directory_id } => Reply::new(library.purge_directory(&directory_id)?),
        ReadyCommand::EmptyTrash => Reply::new(library.empty_trash()?),
        // The new folder identity stays server-side; callers refresh instead.
        ReadyCommand::CreateDirectory { parent_id, name } => Reply::new(library.create_directory(&parent_id, &name).map(|_| ())?),
        ReadyCommand::RenameDirectory { directory_id, name } => Reply::new(library.queue_directory_move(&directory_id, None, Some(&name))?),
        ReadyCommand::MoveDirectory { directory_id, parent_id } => Reply::new(library.queue_directory_move(&directory_id, Some(&parent_id), None)?),
        ReadyCommand::CopyDirectory { directory_id, parent_id } => Reply::new(library.copy_directory(&directory_id, &parent_id)?),
        ReadyCommand::RestoreBookPlacement { content_hash, directory_id } => Reply::new(library.restore_book_placement(&content_hash, &directory_id)?),
        ReadyCommand::CopyBookToDirectory { content_hash, source_id, destination_id } => Reply::new(library.transfer_book_placement(&content_hash, &source_id, &destination_id, false)?),
        ReadyCommand::MoveBookToDirectory { content_hash, source_id, destination_id } => Reply::new(library.transfer_book_placement(&content_hash, &source_id, &destination_id, true)?),
        ReadyCommand::RemoveBookFromDirectory { content_hash, source_id } => Reply::new(library.remove_book_from_directory(&content_hash, &source_id)?),
        ReadyCommand::MoveBookToTrash { content_hash } => Reply::new(library.move_book_to_trash(&content_hash)?),
        ReadyCommand::MoveDirectoryToTrash { directory_id } => Reply::new(library.move_directory_to_trash(&directory_id)?),
        ReadyCommand::Browse { query } => Reply::new(library.db.browse(&query, library.downloaded_books_only()?).map_err(|error| error.to_string())?),
        ReadyCommand::BookDetail { content_hash } => Reply::new(library.db.browse_book_detail(&content_hash).map_err(BackendError::operation)?),
        ReadyCommand::Authors { sort } => Reply::new(library.db.browse_authors(sort, library.downloaded_books_only()?).map_err(BackendError::operation)?),
        ReadyCommand::BookFormat { content_hash } => Reply::new(library.db.book_format(&content_hash).map_err(BackendError::operation)?),
        ReadyCommand::ReadingPosition { content_hash } => Reply::new(library.db.reading_position(&content_hash).map_err(BackendError::operation)?),
        ReadyCommand::PdfReadingPosition { content_hash } => Reply::new(library.db.pdf_reading_position(&content_hash).map_err(BackendError::operation)?),
        // `entry` is the navigation entry the reader resolved the position to. It
        // travels with the position because that is when it changes, and it is
        // stored locally rather than synchronized: any device can recompute it
        // from the position and the book's own navigation.
        ReadyCommand::UpdateReadingPosition { content_hash, position, progress, entry } => Reply::new(library.db.update_reading_position(&content_hash, &position, progress, entry.as_deref()).map_err(BackendError::operation)?),
        ReadyCommand::Annotations { content_hash } => Reply::new(library.db.annotations(content_hash).map_err(BackendError::operation)?),
        ReadyCommand::UpsertAnnotation { annotation } => Reply::new(library.upsert_annotation(&annotation)?),
        ReadyCommand::DeleteAnnotation { annotation_id, modified_at } => Reply::new(library.delete_annotation(&annotation_id, modified_at)?),
        #[cfg(target_arch = "wasm32")]
        ReadyCommand::ReadBookRange { capability, offset, length } => Reply::new(web::read_book_range(&library, &capability, offset, length)?),
        ReadyCommand::CleanupImport { job, files } => Reply::new(library.cleanup_import(&job, files)?),
    })
}

/// Prepares owned work with an explicit concurrency/cancellation class.
fn prepare_pending(library: Rc<LibrarySession>, command: PendingCommand) -> PendingOperation {
    match command {
        PendingCommand::LocalStorageUsage => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(library.local_storage_usage().await?)) })),
        // Renewal only authorizes the original remote book. It must not reopen
        // metadata or change source type if the user downloaded a local copy.
        PendingCommand::RenewRemoteAudio { content_hash } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(library.direct_audio_source(content_hash).await?)) })),
        PendingCommand::ReadFileRange { source, offset, length } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(serde_bytes::ByteBuf::from(library.read_file_range(&source, offset, length).await?))) })),
        PendingCommand::ReadFileBundle { source, ranges } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(serde_bytes::ByteBuf::from(library.read_file_bundle(&source, ranges).await?))) })),
        PendingCommand::Thumbnail { content_hash, resolution } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(library.thumbnail(content_hash, resolution).await?.map(serde_bytes::ByteBuf::from))) })),
        PendingCommand::ImportBook { parent_id, file_name, source } => PendingOperation::new(OperationKind::Import, Box::pin(async move { Ok(Reply::new(library.import_book(parent_id, file_name, source).await?)) })),
        // Staging registers the activity first so a failed preparation cannot
        // leave the import counted as running.
        PendingCommand::PrepareStagedDirectoryImport { activity, job, parent_id, name, directories } => {
            library.begin_directory_import(activity);
            let activity = PreparingImport { library: library.clone(), activity, handed_off: false };
            PendingOperation::new(
                OperationKind::Import,
                Box::pin(async move {
                    let prepared = library.prepare_directory_import_job(&parent_id, name, directories, Some(job)).await?;
                    activity.hand_off();
                    Ok(Reply::new(prepared))
                }),
            )
        }
        PendingCommand::AudiobookPlaybackMetadata { content_hash } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(library.audiobook_playback_metadata(content_hash).await?)) })),
        #[cfg(target_arch = "wasm32")]
        PendingCommand::OpenRemoteAudio { content_hash } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(web::open_remote_audio(&library, content_hash).await?)) })),
    }
}

#[cfg(test)]
pub(crate) async fn dispatch_library_request_in_session(library: Rc<LibrarySession>, command: LibraryCommand) -> Result<Reply, BackendError> {
    let command = match command {
        LibraryCommand::Runtime(command) => return Ok(dispatch_runtime_request(&library.runtime, &library.event_tx, &library.sync_wake, command)),
        LibraryCommand::Session(command) => command,
    };
    match prepare_library_request(library, command)? {
        PreparedRequest::Ready(reply) => Ok(reply),
        PreparedRequest::Pending(operation) => operation.future.await,
    }
}
