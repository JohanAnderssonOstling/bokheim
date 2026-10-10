//! Execution of a library protocol command by its owning session.
//!
//! The runtime defines the wire command, but only the library module is
//! allowed to interpret it against a resource-bearing LibrarySession.

use crate::executor::BackendOutput;
use crate::library::RemoteFile;
pub(crate) use crate::ThumbnailResolution;
use crate::{BackendError, ContentHash, DirId};
pub(crate) use client_runtime::reply::Reply;

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

/// A command for the library actor, classified by how it is scheduled.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum LibraryCommand {
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
    SyncStatus {
        signed_in: bool,
        remote_present: Option<bool>,
    },
    BookCount,
    /// The browser streams books it does not hold; there these are refused.
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
        source_id: Option<DirId>,
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
        annotation: book_model::Annotation,
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
}

/// Commands that prepare owned asynchronous work for the actor scheduler.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum PendingCommand {
    LocalStorageUsage,
    /// Downloads a public-domain catalog book and imports it.
    AddCatalogBook { parent_id: DirId, content_hash: ContentHash },
    /// A file downloaded elsewhere, such as from an OPDS feed, imported as
    /// if it were a local one.
    ImportDownloadedBook { parent_id: DirId, file_name: String, bytes: serde_bytes::ByteBuf },
    #[cfg(not(target_arch = "wasm32"))]
    ImportPaths { parent_id: DirId, paths: Vec<std::path::PathBuf>, moving: bool },
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
    /// A file the caller staged on local disk; the caller deletes it afterwards.
    #[cfg(not(target_arch = "wasm32"))]
    ImportBook {
        parent_id: DirId,
        file_name: String,
        location: std::path::PathBuf,
    },
    /// A folder picked in the browser. Its files arrive with the request,
    /// held under `token`, in the order of `paths`.
    #[cfg(target_arch = "wasm32")]
    ImportSelectedDirectory {
        parent_id: DirId,
        create_root: bool,
        name: String,
        directories: Vec<Vec<String>>,
        paths: Vec<Vec<String>>,
        token: String,
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

/// Executes short commands in receive order, or prepares owned async work.
/// The exhaustive match is the ordering audit: adding an async operation must
/// choose its concurrency/cancellation class explicitly. Native replies stay
/// typed; encoding still belongs only to the worker boundary.
pub(crate) fn prepare_library_request(library: Rc<LibrarySession>, command: LibraryCommand) -> Result<PreparedRequest, BackendError> {
    match command {
        LibraryCommand::Ready(command) => Ok(PreparedRequest::Ready(execute_ready(&library, command)?)),
        LibraryCommand::Pending(command) => Ok(PreparedRequest::Pending(prepare_pending(library, command))),
    }
}

/// Executes synchronous commands in receive order.
fn execute_ready(library: &LibrarySession, command: ReadyCommand) -> Result<Reply, BackendError> {
    Ok(match command {
        ReadyCommand::Scanning => Reply::new(library.scanner_running()),
        ReadyCommand::DownloadState { content_hash } => Reply::new(library.download_state(content_hash)?),
        ReadyCommand::SyncStatus { signed_in, remote_present } => Reply::new(library.sync_status(signed_in, remote_present)?),
        ReadyCommand::BookCount => Reply::new(library.book_count()?),
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::DownloadBook { content_hash } => Reply::new(library.queue_download(content_hash)?),
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::DownloadBrowse { query } => Reply::new(library.queue_browse_downloads(query)?),
        #[cfg(target_arch = "wasm32")]
        ReadyCommand::DownloadBook { .. } | ReadyCommand::DownloadBrowse { .. } => return Err(BackendError::message("Books are not downloaded in the browser")),
        ReadyCommand::RemoveLocalBookCopy { content_hash } => Reply::new(library.remove_local_book_copy(content_hash)?),
        ReadyCommand::EvictFolderDownloads { directory_id } => Reply::new(library.evict_folder_downloads(&directory_id)?),
        ReadyCommand::FolderDestinations => Reply::new(library.db.folder_destinations()?),
        ReadyCommand::Trash => Reply::new(library.db.library_trash()?),
        ReadyCommand::RestoreBook { content_hash } => Reply::new(library.placement.restore_book(&content_hash)?),
        ReadyCommand::RestoreDirectory { directory_id, parent_id } => Reply::new(library.placement.restore_directory(&directory_id, parent_id.as_ref())?),
        ReadyCommand::PurgeBook { content_hash } => Reply::new(library.placement.purge_book(&content_hash)?),
        ReadyCommand::PurgeDirectory { directory_id } => Reply::new(library.placement.purge_directory(&directory_id)?),
        ReadyCommand::EmptyTrash => Reply::new(library.placement.empty_trash()?),
        // The new folder identity stays server-side; callers refresh instead.
        ReadyCommand::CreateDirectory { parent_id, name } => Reply::new(library.placement.create_directory(&parent_id, &name).map(|_| ())?),
        ReadyCommand::RenameDirectory { directory_id, name } => Reply::new(library.placement.queue_directory_move(&directory_id, None, Some(&name))?),
        ReadyCommand::MoveDirectory { directory_id, parent_id } => Reply::new(library.placement.queue_directory_move(&directory_id, Some(&parent_id), None)?),
        ReadyCommand::CopyDirectory { directory_id, parent_id } => Reply::new(library.placement.copy_directory(&directory_id, &parent_id)?),
        ReadyCommand::RestoreBookPlacement { content_hash, directory_id } => Reply::new(library.placement.restore_book_placement(&content_hash, &directory_id)?),
        ReadyCommand::CopyBookToDirectory { content_hash, source_id, destination_id } => Reply::new(library.placement.copy_book_to_directory(&content_hash, source_id.as_ref(), &destination_id)?),
        ReadyCommand::MoveBookToDirectory { content_hash, source_id, destination_id } => Reply::new(library.placement.transfer_book_placement(&content_hash, &source_id, &destination_id, true)?),
        ReadyCommand::RemoveBookFromDirectory { content_hash, source_id } => Reply::new(library.placement.remove_book_from_directory(&content_hash, &source_id)?),
        ReadyCommand::MoveBookToTrash { content_hash } => Reply::new(library.placement.move_book_to_trash(&content_hash)?),
        ReadyCommand::MoveDirectoryToTrash { directory_id } => Reply::new(library.placement.move_directory_to_trash(&directory_id)?),
        ReadyCommand::Browse { query } => Reply::new(library.browse(&query)?),
        ReadyCommand::BookDetail { content_hash } => Reply::new(library.db.browse_book_detail(&content_hash).map_err(BackendError::operation)?),
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
    })
}

/// Prepares owned work with an explicit concurrency/cancellation class.
fn prepare_pending(library: Rc<LibrarySession>, command: PendingCommand) -> PendingOperation {
    match command {
        PendingCommand::LocalStorageUsage => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(library.local_storage_usage().await?)) })),
        PendingCommand::ImportDownloadedBook { parent_id, file_name, bytes } => PendingOperation::new(OperationKind::Import, Box::pin(async move { Ok(Reply::new(library.import.import_downloaded_book(parent_id, &file_name, bytes.into_vec()).await?)) })),
        PendingCommand::AddCatalogBook { parent_id, content_hash } => PendingOperation::new(OperationKind::Import, Box::pin(async move { Ok(Reply::new(library.import.import_catalog_book(parent_id, content_hash).await?)) })),
        // Renewal only authorizes the original remote book. It must not reopen
        // metadata or change source type if the user downloaded a local copy.
        PendingCommand::RenewRemoteAudio { content_hash } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(library.direct_audio_source(content_hash).await?)) })),
        PendingCommand::ReadFileRange { source, offset, length } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(serde_bytes::ByteBuf::from(library.read_file_range(&source, offset, length).await.map_err(BackendError::operation)?))) })),
        PendingCommand::ReadFileBundle { source, ranges } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(serde_bytes::ByteBuf::from(library.read_file_bundle(&source, ranges).await.map_err(BackendError::operation)?))) })),
        PendingCommand::Thumbnail { content_hash, resolution } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(library.thumbnail(content_hash, resolution).await?.map(serde_bytes::ByteBuf::from))) })),
        #[cfg(not(target_arch = "wasm32"))]
        PendingCommand::ImportPaths { parent_id, paths, moving } => PendingOperation::new(OperationKind::Import, Box::pin(async move { Ok(Reply::new(library.import_paths(parent_id, paths, moving).await?)) })),
        #[cfg(not(target_arch = "wasm32"))]
        PendingCommand::ImportBook { parent_id, file_name, location } => PendingOperation::new(OperationKind::Import, Box::pin(async move { Ok(Reply::new(library.import.import_stored_book(parent_id, file_name, location).await?)) })),
        #[cfg(target_arch = "wasm32")]
        PendingCommand::ImportSelectedDirectory { parent_id, create_root, name, directories, paths, token } => PendingOperation::new(
            OperationKind::Import,
            Box::pin(async move {
                use client_platform_web::transport::import_io::{release_selected_files, take_selected_file};
                let files = paths
                    .into_iter()
                    .enumerate()
                    .map(|(index, path)| {
                        let token = token.clone();
                        super::DirectoryImportFile { path, source: super::ImportSource::from_export(Some(Box::new(move || take_selected_file(&token, index)))) }
                    })
                    .collect();
                let result = library.import_selected_directory(parent_id, super::DirectoryImport { name, directories, files }, create_root, &|| false).await;
                release_selected_files(&token);
                Ok(Reply::new(result?))
            }),
        ),
        PendingCommand::AudiobookPlaybackMetadata { content_hash } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(library.audiobook_playback_metadata(content_hash).await?)) })),
        #[cfg(target_arch = "wasm32")]
        PendingCommand::OpenRemoteAudio { content_hash } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(web::open_remote_audio(&library, content_hash).await?)) })),
    }
}
