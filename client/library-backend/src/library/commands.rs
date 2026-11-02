//! Execution of a library protocol command by its owning session.
//!
//! The runtime defines the wire command, but only the library module is
//! allowed to interpret it against a resource-bearing LibrarySession.

pub(crate) use crate::ThumbnailResolution;
use crate::executor::BackendOutput;
use crate::library::RemoteFile;
use crate::{BackendError, BookId, DirId};
pub(crate) use client_runtime::reply::Reply;
use library_model::{BrowseLocation, BrowsePageRequest, LibraryBrowseQuery, LibraryUpdate};

pub(crate) use super::LibrarySession;
use super::handle::LibraryActor;
pub(crate) use super::pending::OperationKind;
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

/// Commands normally executed synchronously by the owning actor. Native
/// placement edits retain their wire shape but prepare owned asynchronous I/O.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum ReadyCommand {
    #[cfg(not(target_arch = "wasm32"))]
    RegisterOpdsBooks {
        books: Vec<library_model::OpdsPublication>,
    },
    #[cfg(not(target_arch = "wasm32"))]
    OpdsBookStates {
        books: Vec<BookId>,
    },
    TextSearchIndexBytes,
    #[cfg(not(target_arch = "wasm32"))]
    ReaderImportProgress { import_id: uuid::Uuid },
    #[cfg(not(target_arch = "wasm32"))]
    CancelReaderImport { import_id: uuid::Uuid },
    Scanning,
    DownloadState {
        content_hash: BookId,
    },
    SyncStatus {
        signed_in: bool,
        remote_present: Option<bool>,
    },
    BookCount,
    LibraryLanguageCounts,
    /// Retain an offline copy of a library book.
    DownloadBook {
        content_hash: BookId,
    },
    DownloadBrowse {
        query: LibraryBrowseQuery,
    },
    RemoveLocalBookCopy {
        content_hash: BookId,
    },
    EvictFolderDownloads {
        directory_id: DirId,
    },
    FolderDestinations,
    Trash,
    RestoreBook {
        content_hash: BookId,
    },
    RestoreDirectory {
        directory_id: DirId,
        parent_id: Option<DirId>,
    },
    PurgeBook {
        content_hash: BookId,
    },
    PurgeDirectory {
        directory_id: DirId,
    },
    EmptyTrash,
    StorageCopyPlan,
    CloudQuotaBooks,
    BookStorageTitles {
        books: Vec<BookId>,
    },
    CheckStorageCopyCapacity {
        bytes: u64,
    },
    StorageCopyBookPresent {
        book: BookId,
        parent_id: DirId,
    },
    EnsureStorageCopyDirectory {
        id: DirId,
        parent_id: DirId,
        name: String,
    },
    #[cfg(not(target_arch = "wasm32"))]
    TransferPlan {
        selection: library_database::transfer::TransferSelection,
    },
    #[cfg(not(target_arch = "wasm32"))]
    RemoveTransferredSelection {
        plan: library_database::transfer::TransferPlan,
    },
    #[cfg(not(target_arch = "wasm32"))]
    CreateDirectoryWithId {
        parent_id: DirId,
        name: String,
    },
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
        content_hash: BookId,
        directory_id: DirId,
    },
    CopyBookToDirectory {
        content_hash: BookId,
        source_id: Option<DirId>,
        destination_id: DirId,
    },
    RemoveBookFromDirectory {
        content_hash: BookId,
        source_id: DirId,
    },
    MoveBookToTrash {
        content_hash: BookId,
    },
    MoveDirectoryToTrash {
        directory_id: DirId,
    },
    Browse {
        query: LibraryBrowseQuery,
    },
    BrowseResultPage {
        query: LibraryBrowseQuery,
        revision: String,
        page: BrowsePageRequest,
    },
    BrowseResultBooks {
        query: LibraryBrowseQuery,
        revision: String,
        section: BrowseLocation,
        offset: usize,
        limit: usize,
    },
    BrowsePage {
        query: LibraryBrowseQuery,
        page: BrowsePageRequest,
    },
    BrowseSectionBooks {
        query: LibraryBrowseQuery,
        offset: usize,
        limit: usize,
    },
    BookDetail {
        content_hash: BookId,
    },
    ExternalCatalogDetail {
        entry: library_model::CardKey,
    },
    BookFormat {
        content_hash: BookId,
    },
    ReadingPosition {
        content_hash: BookId,
    },
    PdfReadingPosition {
        content_hash: BookId,
    },
    ResetChapterReadStatus {
        content_hash: BookId,
        target: Option<String>,
    },
    UpdateReadingPosition {
        content_hash: BookId,
        position: String,
        progress: Option<f32>,
        entry: Option<String>,
    },
    SetBookRead {
        content_hash: BookId,
        read: bool,
    },
    Annotations {
        content_hash: BookId,
    },
    Annotation {
        content_hash: BookId,
        annotation_id: String,
    },
    LibraryRecurringAnnotations { excluding: BookId },
    Notebook { query: library_model::NotebookQuery, request: library_model::NotebookRequest },
    NotebookAnnotations { query: library_model::NotebookQuery, revision: String, book: BookId, offset: usize },
    UpsertAnnotation {
        annotation: book_model::Annotation,
    },
    DeleteAnnotation {
        annotation_id: String,
        modified_at: i64,
    },
    MigrateEpubAnnotation {
        annotation_id: String,
        legacy_cfi: String,
        publication_cfi: String,
    },
    #[cfg(target_arch = "wasm32")]
    ReadBookRange {
        capability: String,
        offset: u64,
        length: usize,
    },
    MoveBookPlacement {
        content_hash: BookId,
        source_id: DirId,
        destination_id: DirId,
    },
    ResolveBookIdentity { content_hash: BookId },
    #[cfg(not(target_arch = "wasm32"))]
    AnnotationExportPlan { options: library_model::AnnotationExportOptions },
    #[cfg(not(target_arch = "wasm32"))]
    AnnotationExportProgress { export_id: uuid::Uuid },
    #[cfg(not(target_arch = "wasm32"))]
    CancelAnnotationExport { export_id: uuid::Uuid },
    AnnotationExportSnapshot,
    #[cfg(not(target_arch = "wasm32"))]
    AutomaticAnnotationExport,
    #[cfg(not(target_arch = "wasm32"))]
    SetAutomaticAnnotationExport { options: Option<library_model::AnnotationExportOptions> },
    #[cfg(not(target_arch = "wasm32"))]
    RetryAutomaticAnnotationExport,
}

/// Commands that prepare owned asynchronous work for the actor scheduler.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum PendingCommand {
    #[cfg(not(target_arch = "wasm32"))]
    OpdsCover {
        entry: library_model::CardKey,
        width: u32,
        card: bool,
    },
    TextSearch {
        query: library_model::NotebookQuery,
        request: library_model::NotebookRequest,
    },
    TextSearchMatches {
        query: library_model::NotebookQuery,
        revision: String,
        book: BookId,
        offset: usize,
        limit: usize,
    },
    #[cfg(not(target_arch = "wasm32"))]
    ReaderImportSummary { source: crate::ReaderImportSource },
    #[cfg(not(target_arch = "wasm32"))]
    ImportReader { import_id: uuid::Uuid, source: crate::ReaderImportSource, options: crate::ReaderImportOptions },
    RefreshStorageCopy,
    LocalStorageUsage,
    /// Downloads a public-domain catalog book and imports it.
    AddCatalogBook {
        parent_id: DirId,
        content_hash: BookId,
    },
    /// Downloads an external catalog audiobook from its source and imports it.
    AddExternalCatalogAudiobook {
        parent_id: DirId,
        entry: library_model::CardKey,
    },
    /// A file downloaded elsewhere, such as from an OPDS feed, imported as
    /// if it were a local one.
    ImportDownloadedBook {
        parent_id: DirId,
        file_name: String,
        bytes: serde_bytes::ByteBuf,
    },
    #[cfg(not(target_arch = "wasm32"))]
    ImportPaths {
        parent_id: DirId,
        paths: Vec<std::path::PathBuf>,
        moving: bool,
    },
    // Renewal only authorizes the original remote book. It must not reopen
    // metadata or change source type if the user downloaded a local copy.
    RenewRemoteAudio {
        content_hash: BookId,
    },
    ReadFileRange {
        source: RemoteFile,
        offset: u64,
        length: usize,
    },
    /// Bounded, exact multipart ranges for book opening.
    ReadFileRanges {
        source: RemoteFile,
        ranges: Vec<(u64, usize)>,
    },
    ReadFileBundle {
        source: RemoteFile,
        ranges: Vec<(u64, usize)>,
    },
    Thumbnail {
        content_hash: BookId,
        resolution: ThumbnailResolution,
    },
    ExternalCatalogCover {
        entry: library_model::CardKey,
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
        content_hash: BookId,
    },
    #[cfg(target_arch = "wasm32")]
    OpenRemoteAudio {
        content_hash: BookId,
    },
    #[cfg(not(target_arch = "wasm32"))]
    ExportAnnotations { export_id: uuid::Uuid, options: library_model::AnnotationExportOptions },
    #[cfg(not(target_arch = "wasm32"))]
    AnnotationExportRunningReaders { formats: Vec<library_model::AnnotationExportFormat> },
}

/// A serialized reply produced by a library command.
pub trait LibraryReply: BackendOutput + serde::Serialize + serde::de::DeserializeOwned {}
impl<T: BackendOutput + serde::Serialize + serde::de::DeserializeOwned> LibraryReply for T {}

/// Executes short commands in receive order, or prepares owned async work.
/// The exhaustive match is the ordering audit: adding an async operation must
/// choose its concurrency/cancellation class explicitly. Native replies stay
/// typed; encoding still belongs only to the worker boundary.
pub(crate) fn prepare_library_request(actor: &mut LibraryActor, command: LibraryCommand) -> Result<PreparedRequest, BackendError> {
    #[cfg(not(target_arch = "wasm32"))]
    if actor.session.is_opds() {
        if let LibraryCommand::Ready(ReadyCommand::DownloadBook { content_hash }) = &command {
            let library = actor.session.clone();
            let id = *content_hash;
            return Ok(PreparedRequest::Pending(PendingOperation::new(OperationKind::Import, Box::pin(async move { Ok(Reply::new(library.download_opds(id).await?)) }))));
        }
        // Personal state and local availability remain writable; content editing is not exposed.
        let allowed = match &command {
            LibraryCommand::Ready(command) => matches!(
                command,
                ReadyCommand::RegisterOpdsBooks { .. }
                    | ReadyCommand::OpdsBookStates { .. }
                    | ReadyCommand::TextSearchIndexBytes
                    | ReadyCommand::CloudQuotaBooks
                    | ReadyCommand::BookStorageTitles { .. }
                    | ReadyCommand::Scanning
                    | ReadyCommand::DownloadState { .. }
                    | ReadyCommand::SyncStatus { .. }
                    | ReadyCommand::BookCount
                    | ReadyCommand::LibraryLanguageCounts
                    | ReadyCommand::RemoveLocalBookCopy { .. }
                    | ReadyCommand::Browse { .. }
                    | ReadyCommand::BrowseResultPage { .. }
                    | ReadyCommand::BrowseResultBooks { .. }
                    | ReadyCommand::BrowsePage { .. }
                    | ReadyCommand::BrowseSectionBooks { .. }
                    | ReadyCommand::BookDetail { .. }
                    | ReadyCommand::BookFormat { .. }
                    | ReadyCommand::ReadingPosition { .. }
                    | ReadyCommand::PdfReadingPosition { .. }
                    | ReadyCommand::ResetChapterReadStatus { .. }
                    | ReadyCommand::UpdateReadingPosition { .. }
                    | ReadyCommand::SetBookRead { .. }
                    | ReadyCommand::Annotations { .. }
                    | ReadyCommand::Annotation { .. }
                    | ReadyCommand::LibraryRecurringAnnotations { .. }
                    | ReadyCommand::Notebook { .. }
                    | ReadyCommand::NotebookAnnotations { .. }
                    | ReadyCommand::UpsertAnnotation { .. }
                    | ReadyCommand::DeleteAnnotation { .. }
                    | ReadyCommand::MigrateEpubAnnotation { .. }
                    | ReadyCommand::ResolveBookIdentity { .. }
                    | ReadyCommand::AnnotationExportSnapshot
                    | ReadyCommand::AutomaticAnnotationExport
            ),
            LibraryCommand::Pending(command) => {
                matches!(command, PendingCommand::OpdsCover { .. } | PendingCommand::LocalStorageUsage | PendingCommand::TextSearch { .. } | PendingCommand::TextSearchMatches { .. } | PendingCommand::Thumbnail { .. })
            }
        };
        if !allowed {
            return Err("this OPDS library is read only".into());
        }
    }
    match command {
        #[cfg(not(target_arch = "wasm32"))]
        LibraryCommand::Ready(
            command @ (ReadyCommand::CopyDirectory { .. }
            | ReadyCommand::CopyBookToDirectory { .. }
            | ReadyCommand::MoveBookPlacement { .. }
            | ReadyCommand::RestoreBookPlacement { .. }
            | ReadyCommand::RenameDirectory { .. }
            | ReadyCommand::MoveDirectory { .. }),
        ) => {
            let library = actor.session.clone();
            Ok(PreparedRequest::Pending(PendingOperation::new(
                OperationKind::Import,
                Box::pin(async move {
                    let _activity = library.directory_import_activity();
                    Ok(match command {
                        ReadyCommand::CopyDirectory { directory_id, parent_id } => Reply::new(library.placement.copy_directory(&directory_id, &parent_id).await?),
                        ReadyCommand::CopyBookToDirectory { content_hash, source_id, destination_id } => Reply::new(library.placement.copy_book_to_directory(&content_hash, source_id.as_ref(), &destination_id).await?),
                        ReadyCommand::MoveBookPlacement { content_hash, source_id, destination_id } => Reply::new(library.placement.move_book_placement(&content_hash, &source_id, &destination_id).await?),
                        ReadyCommand::RestoreBookPlacement { content_hash, directory_id } => Reply::new(library.placement.restore_book_placement(&content_hash, &directory_id).await?),
                        ReadyCommand::RenameDirectory { directory_id, name } => Reply::new(library.placement.queue_directory_move(&directory_id, None, Some(&name)).await?),
                        ReadyCommand::MoveDirectory { directory_id, parent_id } => Reply::new(library.placement.queue_directory_move(&directory_id, Some(&parent_id), None).await?),
                        _ => unreachable!(),
                    })
                }),
            )))
        }
        LibraryCommand::Ready(command) => Ok(PreparedRequest::Ready(execute_ready(actor, command)?)),
        LibraryCommand::Pending(command) => Ok(PreparedRequest::Pending(prepare_pending(actor.session.clone(), command))),
    }
}

/// Executes synchronous commands in receive order.
fn execute_ready(actor: &mut LibraryActor, command: ReadyCommand) -> Result<Reply, BackendError> {
    let library = &actor.session;
    Ok(match command {
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::RegisterOpdsBooks { books } => Reply::new(library.register_opds_books(books)?),
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::OpdsBookStates { books } => Reply::new(library.opds_book_states(books)?),
        ReadyCommand::TextSearchIndexBytes => {
            if !client_platform_runtime::text_search::enabled() {
                library.db.configure_text_index(false)?;
            }
            Reply::new(library.db.text_index_bytes()?)
        }
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::AnnotationExportPlan {options} => Reply::new(library.annotation_export_plan(options)?),
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::ReaderImportProgress { import_id } => Reply::new(library.reader_import_progress(import_id)),
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::CancelReaderImport { import_id } => Reply::new(library.cancel_reader_import(import_id)),
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::AnnotationExportProgress {export_id} => Reply::new(library.annotation_export_progress(export_id)),
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::CancelAnnotationExport {export_id} => Reply::new(library.cancel_annotation_export(export_id)),
        ReadyCommand::AnnotationExportSnapshot => Reply::new(library.db.annotation_export_snapshot()?),
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::AutomaticAnnotationExport => Reply::new(library.automatic_annotation_export()?),
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::SetAutomaticAnnotationExport { options } => Reply::new(library.set_automatic_annotation_export(options)?),
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::RetryAutomaticAnnotationExport => Reply::new(library.retry_automatic_annotation_export()?),
        ReadyCommand::Scanning => Reply::new(library.scanner_running()),
        ReadyCommand::DownloadState { content_hash } => Reply::new(library.download_state(&actor.coordination, content_hash)?),
        ReadyCommand::SyncStatus { signed_in, remote_present } => Reply::new(library.sync_status(signed_in, remote_present)?),
        ReadyCommand::BookCount => Reply::new(library.db.book_count().map_err(BackendError::operation)?),
        ReadyCommand::LibraryLanguageCounts => Reply::new(library.db.library_language_counts().map_err(BackendError::operation)?),
        ReadyCommand::DownloadBook { content_hash } => Reply::new(library.queue_download(&mut actor.coordination, content_hash)?),
        ReadyCommand::DownloadBrowse { query } => Reply::new(library.queue_browse_downloads(query)?),
        ReadyCommand::RemoveLocalBookCopy { content_hash } => Reply::new(library.remove_local_book_copy(content_hash)?),
        ReadyCommand::EvictFolderDownloads { directory_id } => Reply::new(library.evict_folder_downloads(&directory_id)?),
        ReadyCommand::FolderDestinations => Reply::new(library.db.folder_destinations()?),
        ReadyCommand::Trash => Reply::new(library.db.library_trash()?),
        ReadyCommand::RestoreBook { content_hash } => Reply::new(library.placement.restore_book(&content_hash)?),
        ReadyCommand::RestoreDirectory { directory_id, parent_id } => Reply::new(library.placement.restore_directory(&directory_id, parent_id.as_ref())?),
        ReadyCommand::PurgeBook { content_hash } => Reply::new(library.placement.purge_book(&content_hash)?),
        ReadyCommand::PurgeDirectory { directory_id } => Reply::new(library.placement.purge_directory(&directory_id)?),
        ReadyCommand::EmptyTrash => Reply::new(library.placement.empty_trash()?),
        ReadyCommand::StorageCopyPlan => Reply::new(library.db.storage_copy_plan()?),
        ReadyCommand::CloudQuotaBooks => Reply::new(library.db.cloud_quota_books()?),
        ReadyCommand::BookStorageTitles { books } => Reply::new(library.db.book_storage_titles(&books)?),
        ReadyCommand::CheckStorageCopyCapacity { bytes } => {
            library.assets.check_write_capacity(bytes).map_err(BackendError::operation)?;
            Reply::new(())
        }
        ReadyCommand::StorageCopyBookPresent { book, parent_id } => {
            Reply::new(library.db.transfer_destination_name(&book, &parent_id)?.is_some() && library.assets.book_exists(&book, &library.db.book_paths(&book)?).map_err(BackendError::operation)?)
        }
        ReadyCommand::EnsureStorageCopyDirectory { id, parent_id, name } => {
            if let Some(existing) = library.db.folder_destinations()?.into_iter().find(|folder| folder.id == id) {
                if existing.parent_id != Some(parent_id) || existing.label != name {
                    return Err(BackendError::message("copied folder was changed; keep the existing copy and choose another destination"));
                }
            } else {
                library.db.create_directory_with_id(&id, &parent_id, &name)?;
                library.events.notify_contents_changed();
            }
            Reply::new(())
        }
        // The new folder identity stays server-side; callers refresh instead.
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::TransferPlan { selection } => Reply::new(library.db.transfer_plan(selection)?),
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::RemoveTransferredSelection { plan } => {
            library.db.remove_transferred_selection(&plan)?;
            library.events.notify_contents_changed();
            Reply::new(())
        }
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::CreateDirectoryWithId { parent_id, name } => Reply::new(library.placement.create_directory(&parent_id, &name)?.id),
        ReadyCommand::CreateDirectory { parent_id, name } => Reply::new(library.placement.create_directory(&parent_id, &name).map(|_| ())?),
        #[cfg(target_arch = "wasm32")]
        ReadyCommand::RenameDirectory { directory_id, name } => Reply::new(library.placement.queue_directory_move_metadata(&directory_id, None, Some(&name))?),
        #[cfg(target_arch = "wasm32")]
        ReadyCommand::MoveDirectory { directory_id, parent_id } => Reply::new(library.placement.queue_directory_move_metadata(&directory_id, Some(&parent_id), None)?),
        #[cfg(target_arch = "wasm32")]
        ReadyCommand::CopyDirectory { directory_id, parent_id } => Reply::new(library.placement.copy_directory_metadata(&directory_id, &parent_id)?),
        #[cfg(not(target_arch = "wasm32"))]
        ReadyCommand::CopyDirectory { .. }
        | ReadyCommand::CopyBookToDirectory { .. }
        | ReadyCommand::MoveBookPlacement { .. }
        | ReadyCommand::RestoreBookPlacement { .. }
        | ReadyCommand::RenameDirectory { .. }
        | ReadyCommand::MoveDirectory { .. } => unreachable!("native placement requests are prepared asynchronously"),
        #[cfg(target_arch = "wasm32")]
        ReadyCommand::RestoreBookPlacement { content_hash, directory_id } => Reply::new(library.placement.restore_book_placement_metadata(&content_hash, &directory_id)?),
        #[cfg(target_arch = "wasm32")]
        ReadyCommand::CopyBookToDirectory { content_hash, source_id, destination_id } => Reply::new(library.placement.copy_book_to_directory_metadata(&content_hash, source_id.as_ref(), &destination_id)?),
        #[cfg(target_arch = "wasm32")]
        ReadyCommand::MoveBookPlacement { content_hash, source_id, destination_id } => Reply::new(library.placement.move_book_placement_metadata(&content_hash, &source_id, &destination_id)?),
        ReadyCommand::RemoveBookFromDirectory { content_hash, source_id } => Reply::new(library.placement.remove_book_from_directory(&content_hash, &source_id)?),
        ReadyCommand::MoveBookToTrash { content_hash } => Reply::new(library.placement.move_book_to_trash(&content_hash)?),
        ReadyCommand::MoveDirectoryToTrash { directory_id } => Reply::new(library.placement.move_directory_to_trash(&directory_id)?),
        ReadyCommand::Browse { query } => Reply::new(library.db.browse(&query, library.downloaded_books_only()?).map_err(BackendError::operation)?),
        ReadyCommand::BrowseResultPage { query, revision, page } => Reply::new(library.db.browse_result_page(&query, library.downloaded_books_only()?, &revision, page).map_err(BackendError::operation)?),
        ReadyCommand::BrowseResultBooks { query, revision, section, offset, limit } => {
            Reply::new(library.db.browse_result_books(&query, library.downloaded_books_only()?, &revision, section, offset, limit).map_err(BackendError::operation)?)
        }
        ReadyCommand::BrowsePage { query, page } => Reply::new(library.db.browse_page(&query, library.downloaded_books_only()?, page).map_err(BackendError::operation)?),
        ReadyCommand::BrowseSectionBooks { query, offset, limit } => Reply::new(library.db.browse_section_books(&query, library.downloaded_books_only()?, offset, limit).map_err(BackendError::operation)?),
        ReadyCommand::BookDetail { content_hash } => {
            let mut detail = library.db.browse_book_detail(&content_hash).map_err(BackendError::operation)?;
            if library.is_opds() {
                if let Some(book) = library.db.opds_publication(content_hash)? {
                    detail.description = book.description;
                }
            }
            Reply::new(detail)
        }
        ReadyCommand::ExternalCatalogDetail { entry } => Reply::new(library.db.external_catalog_detail(entry).map_err(BackendError::operation)?),
        ReadyCommand::ResolveBookIdentity { content_hash } => Reply::new(library.db.resolve_book_identity(content_hash).map_err(BackendError::operation)?),
        ReadyCommand::BookFormat { content_hash } => Reply::new(library.db.book_format(&content_hash).map_err(BackendError::operation)?),
        ReadyCommand::ReadingPosition { content_hash } => Reply::new(library.db.reading_position(&content_hash).map_err(BackendError::operation)?),
        ReadyCommand::PdfReadingPosition { content_hash } => Reply::new(library.db.pdf_reading_position(&content_hash).map_err(BackendError::operation)?),
        ReadyCommand::ResetChapterReadStatus { content_hash, target } => {
            if library.db.reset_chapter_read_status_at(&content_hash, target.as_deref()).map_err(BackendError::operation)? {
                actor.coordination.reading_position_changed(content_hash, &library.events);
                library.events.publish_update(LibraryUpdate::ReadingProgress(content_hash));
            }
            Reply::new(library.db.chapter_read_status(&content_hash).map_err(BackendError::operation)?)
        }
        // `entry` is the navigation entry the reader resolved the position to. It
        // travels with the position because that is when it changes, and it is
        // stored locally rather than synchronized: any device can recompute it
        // from the position and the book's own navigation.
        ReadyCommand::UpdateReadingPosition { content_hash, position, progress, entry } => {
            if library.db.update_reading_position(&content_hash, &position, progress, entry.as_deref()).map_err(BackendError::operation)? {
                actor.coordination.reading_position_changed(content_hash, &library.events);
                // Shelf membership, recency and progress change locally, before sync.
                library.events.publish_update(LibraryUpdate::ReadingProgress(content_hash));
            }
            Reply::new(())
        }
        ReadyCommand::SetBookRead { content_hash, read } => {
            if library.db.set_book_read(&content_hash, read).map_err(BackendError::operation)? {
                actor.coordination.reading_position_changed(content_hash, &library.events);
                library.events.publish_update(LibraryUpdate::ReadingProgress(content_hash));
            }
            Reply::new(())
        }
        ReadyCommand::Annotations { content_hash } => Reply::new(library.db.annotations(content_hash).map_err(BackendError::operation)?),
        ReadyCommand::Annotation { content_hash, annotation_id } => Reply::new(library.db.annotation(content_hash, &annotation_id).map_err(BackendError::operation)?),
        ReadyCommand::LibraryRecurringAnnotations { excluding } => Reply::new(library.db.library_recurring_annotations(excluding).map_err(BackendError::operation)?),
        ReadyCommand::Notebook { query, request } => Reply::new(library.db.notebook(&query, &request, library.downloaded_books_only()?).map_err(BackendError::operation)?),
        ReadyCommand::NotebookAnnotations { query, revision, book, offset } => Reply::new(library.db.notebook_annotations(&query, &revision, book, offset, library.downloaded_books_only()?).map_err(BackendError::operation)?),
        ReadyCommand::UpsertAnnotation { annotation } => {
            library.db.upsert_annotation(&annotation).map_err(BackendError::operation)?;
            library.events.publish_update(LibraryUpdate::Annotations);
            #[cfg(not(target_arch = "wasm32"))]
            library.events.annotation_exports.notify_one();
            Reply::new(())
        }
        ReadyCommand::DeleteAnnotation { annotation_id, modified_at } => {
            library.db.delete_annotation(&annotation_id, modified_at).map_err(BackendError::operation)?;
            library.events.publish_update(LibraryUpdate::Annotations);
            #[cfg(not(target_arch = "wasm32"))]
            library.events.annotation_exports.notify_one();
            Reply::new(())
        }
        ReadyCommand::MigrateEpubAnnotation { annotation_id, legacy_cfi, publication_cfi } => {
            library.db.migrate_epub_annotation(&annotation_id, &legacy_cfi, &publication_cfi).map_err(BackendError::operation)?;
            library.events.publish_update(LibraryUpdate::Annotations);
            #[cfg(not(target_arch = "wasm32"))]
            library.events.annotation_exports.notify_one();
            Reply::new(())
        }
        #[cfg(target_arch = "wasm32")]
        ReadyCommand::ReadBookRange { capability, offset, length } => Reply::new(web::read_book_range(&library, &capability, offset, length)?),
    })
}

/// Prepares owned work with an explicit concurrency/cancellation class.
fn prepare_pending(library: Rc<LibrarySession>, command: PendingCommand) -> PendingOperation {
    match command {
        #[cfg(not(target_arch = "wasm32"))]
        PendingCommand::OpdsCover { entry, width, card } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(library.opds_cover(entry, width, card).await?.map(serde_bytes::ByteBuf::from))) })),
        PendingCommand::TextSearch { query, request } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(library.text_search(query, request).await?)) })),
        PendingCommand::TextSearchMatches { query, revision, book, offset, limit } => {
            PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(library.text_search_matches(query, revision, book, offset, limit).await?)) }))
        }
        #[cfg(not(target_arch = "wasm32"))]
        PendingCommand::AnnotationExportRunningReaders { formats } => PendingOperation::new(
            OperationKind::Read,
            Box::pin(async move {
                let running = crate::executor::run_blocking(move || library_annotation_export::running_readers(&formats)).await.map_err(BackendError::operation)?;
                Ok(Reply::new(running))
            }),
        ),
        #[cfg(not(target_arch = "wasm32"))]
        PendingCommand::ReaderImportSummary { source } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(library.reader_import_summary(source).await?)) })),
        #[cfg(not(target_arch = "wasm32"))]
        PendingCommand::ImportReader { import_id, source, options } => {
            let job = library.prepare_reader_import(import_id);
            PendingOperation::new(OperationKind::Import, Box::pin(async move { Ok(Reply::new(library.import_reader(source, options, job?).await?)) }))
        }
        #[cfg(not(target_arch = "wasm32"))]
        PendingCommand::ExportAnnotations {export_id,options} => {
            let job=library.prepare_annotation_export(export_id);
            PendingOperation::new(OperationKind::Import,Box::pin(async move {Ok(Reply::new(library.export_annotations(options,job?).await?))}))
        },
        PendingCommand::RefreshStorageCopy => PendingOperation::new(
            OperationKind::Read,
            Box::pin(async move {
                library.account.require().map_err(BackendError::operation)?;

                super::workers::ensure_remote_library(&library.sync, &library.db).await?;
                library.sync.synchronize_state_mode(false).await.map_err(BackendError::operation)?;
                Ok(Reply::new(()))
            }),
        ),
        PendingCommand::LocalStorageUsage => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(library.local_storage_usage().await?)) })),
        PendingCommand::ImportDownloadedBook { parent_id, file_name, bytes } => {
            PendingOperation::new(OperationKind::Import, Box::pin(async move { Ok(Reply::new(library.import.import_downloaded_book(parent_id, &file_name, bytes.into_vec()).await?)) }))
        }
        PendingCommand::AddCatalogBook { parent_id, content_hash } => PendingOperation::new(OperationKind::Import, Box::pin(async move { Ok(Reply::new(library.import.import_catalog_book(parent_id, content_hash).await?)) })),
        PendingCommand::AddExternalCatalogAudiobook { parent_id, entry } => PendingOperation::new(OperationKind::Import, Box::pin(async move { Ok(Reply::new(library.import.import_external_catalog_audiobook(parent_id, entry).await?)) })),
        // Renewal only authorizes the original remote book. It must not reopen
        // metadata or change source type if the user downloaded a local copy.
        PendingCommand::RenewRemoteAudio { content_hash } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(library.direct_audio_source(content_hash).await?)) })),
        PendingCommand::ReadFileRange { source, offset, length } => {
            PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(serde_bytes::ByteBuf::from(library.read_file_range(&source, offset, length).await.map_err(super::remote_access::range_backend_error)?))) }))
        }
        PendingCommand::ReadFileRanges { source, ranges } => {
            PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(serde_bytes::ByteBuf::from(library.read_file_ranges(&source, ranges).await.map_err(super::remote_access::range_backend_error)?))) }))
        }
        PendingCommand::ReadFileBundle { source, ranges } => {
            PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(serde_bytes::ByteBuf::from(library.read_file_bundle(&source, ranges).await.map_err(super::remote_access::range_backend_error)?))) }))
        }
        PendingCommand::Thumbnail { content_hash, resolution } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(library.thumbnail(content_hash, resolution).await?.map(serde_bytes::ByteBuf::from))) })),
        PendingCommand::ExternalCatalogCover { entry, resolution } => PendingOperation::new(
            OperationKind::Read,
            Box::pin(async move {
                let cover = match resolution {
                    ThumbnailResolution::HighDensity => library_catalog::external_cover(entry, true).await,
                    ThumbnailResolution::Browse | ThumbnailResolution::CardHighDensity => library_catalog::card_cover(entry, resolution.width()).await,
                };
                Ok(Reply::new(cover.map(serde_bytes::ByteBuf::from)))
            }),
        ),
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
        PendingCommand::OpenRemoteAudio { content_hash } => PendingOperation::new(OperationKind::Read, Box::pin(async move { Ok(Reply::new(super::browser_audiobook::open_remote_audio(&library, content_hash).await?)) })),
    }
}
