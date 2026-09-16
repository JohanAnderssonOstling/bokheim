//! Execution of a library protocol command by its owning session.
//!
//! The runtime defines the wire command, but only the library module is
//! allowed to interpret it against a resource-bearing LibrarySession.

use crate::runtime::{HostLibraryCommand, ThumbnailResolution, WorkerPayload, encode_reply};

use super::LibrarySession;
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum LibraryCommand {
    // Renewal only authorizes the original remote book. It must not reopen
    // metadata or change source type if the user downloaded a local copy.
    RenewRemoteAudio {
        content_hash: crate::ContentHash,
    },
    ReadFileRange {
        source: crate::library::RemoteFile,
        offset: u64,
        length: usize,
    },
    ReadFileBundle {
        source: crate::library::RemoteFile,
        ranges: Vec<(u64, usize)>,
    },
    BookCount,
    Home {
        limit: i32,
    },
    Thumbnail {
        content_hash: crate::ContentHash,
        resolution: ThumbnailResolution,
    },
    DownloadBook {
        content_hash: crate::ContentHash,
    },
    DownloadFolder {
        query: library_model::LibraryBrowseQuery,
    },
    DownloadSubject {
        query: library_model::LibraryBrowseQuery,
    },
    FolderDestinations,
    Trash,
    RestoreBook {
        content_hash: crate::ContentHash,
    },
    RestoreDirectory {
        directory_id: crate::DirId,
        parent_id: Option<crate::DirId>,
    },
    PurgeBook {
        content_hash: crate::ContentHash,
    },
    PurgeDirectory {
        directory_id: crate::DirId,
    },
    EmptyTrash,
    CreateDirectory {
        parent_id: crate::DirId,
        name: String,
    },
    RenameDirectory {
        directory_id: crate::DirId,
        name: String,
    },
    MoveDirectory {
        directory_id: crate::DirId,
        parent_id: crate::DirId,
    },
    CopyDirectory {
        directory_id: crate::DirId,
        parent_id: crate::DirId,
    },
    RestoreBookPlacement {
        content_hash: crate::ContentHash,
        directory_id: crate::DirId,
    },
    CopyBookToDirectory {
        content_hash: crate::ContentHash,
        source_id: crate::DirId,
        destination_id: crate::DirId,
    },
    MoveBookToDirectory {
        content_hash: crate::ContentHash,
        source_id: crate::DirId,
        destination_id: crate::DirId,
    },
    RemoveBookFromDirectory {
        content_hash: crate::ContentHash,
        source_id: crate::DirId,
    },
    MoveBookToTrash {
        content_hash: crate::ContentHash,
    },
    MoveDirectoryToTrash {
        directory_id: crate::DirId,
    },
    ImportBookBytes {
        parent_id: crate::DirId,
        file_name: String,
        #[serde(with = "serde_bytes")]
        bytes: Vec<u8>,
    },
    PrepareStagedDirectoryImport {
        activity: uuid::Uuid,
        job: String,
        parent_id: crate::DirId,
        name: Option<String>,
        directories: Vec<Vec<String>>,
    },
    FinishDirectoryImport {
        activity: uuid::Uuid,
    },
    DirectoryImportProgress,
    AdvanceDirectoryImport {
        activity: uuid::Uuid,
        total: u64,
        succeeded: u64,
        failed: u64,
    },
    FolderContents {
        query: library_model::LibraryBrowseQuery,
    },
    SubjectContents {
        query: library_model::LibraryBrowseQuery,
    },
    BookDetail {
        content_hash: crate::ContentHash,
    },
    Authors {
        sort: library_model::AuthorSort,
    },
    BookFormat {
        content_hash: crate::ContentHash,
    },
    AudiobookPlaybackMetadata {
        content_hash: crate::ContentHash,
    },
    ReadingPosition {
        content_hash: crate::ContentHash,
    },
    PdfReadingPosition {
        content_hash: crate::ContentHash,
    },
    UpdateReadingPosition {
        content_hash: crate::ContentHash,
        position: String,
        progress: Option<f32>,
        entry: Option<String>,
    },
    Annotations {
        content_hash: crate::ContentHash,
    },
    UpsertAnnotation {
        annotation: library_model::ReaderAnnotation,
    },
    DeleteAnnotation {
        annotation_id: String,
        modified_at: i64,
    },
    EnrichmentStatus,
    #[serde(untagged)]
    Host(HostLibraryCommand),
}

pub(crate) mod library_requests {
    pub(crate) use super::LibraryCommand::*;
}

/// A serialized reply produced by a library command.
pub(crate) trait LibraryReply: crate::executor::BackendOutput + serde::Serialize + serde::de::DeserializeOwned {}
impl<T: crate::executor::BackendOutput + serde::Serialize + serde::de::DeserializeOwned> LibraryReply for T {}

/// A typed request addressed to one already-open library.
pub(crate) trait LibraryRequest: crate::executor::BackendSend + 'static {
    fn command(self) -> LibraryCommand;
}

impl LibraryRequest for LibraryCommand {
    fn command(self) -> LibraryCommand {
        self
    }
}

/// Runs one command against its owning session and encodes the reply.
///
/// Each arm names the session operation and nothing else: error conversion is
/// `?` through `From<BackendError> for String`, and every reply — including the
/// ones carrying bytes — leaves as a single encoded [`WorkerPayload`].
pub(crate) async fn dispatch_library_request_in_session(library: &LibrarySession, command: LibraryCommand) -> Result<WorkerPayload, String> {
    match command {
        LibraryCommand::Host(command) => dispatch_host_command(command, library).await,
        // Renewal only authorizes the original remote book. It must not reopen
        // metadata or change source type if the user downloaded a local copy.
        LibraryCommand::RenewRemoteAudio { content_hash } => encode_reply(library.direct_audio_source(content_hash).await?),
        LibraryCommand::ReadFileRange { source, offset, length } => encode_reply(serde_bytes::ByteBuf::from(library.read_file_range(&source, offset, length).await?)),
        LibraryCommand::ReadFileBundle { source, ranges } => encode_reply(serde_bytes::ByteBuf::from(library.read_file_bundle(&source, ranges).await?)),
        LibraryCommand::BookCount => encode_reply(library.book_count()?),
        LibraryCommand::Home { limit } => encode_reply(library.home(limit, library.downloaded_books_only()?)?),
        LibraryCommand::Thumbnail { content_hash, resolution } => encode_reply(library.thumbnail(content_hash, resolution).await?.map(serde_bytes::ByteBuf::from)),
        LibraryCommand::DownloadBook { content_hash } => encode_reply(library.request_download(content_hash).await?),
        LibraryCommand::DownloadFolder { query } => encode_reply(library.queue_browse_downloads(query, false)?),
        LibraryCommand::DownloadSubject { query } => encode_reply(library.queue_browse_downloads(query, true)?),
        LibraryCommand::FolderDestinations => encode_reply(library.folder_destinations()?),
        LibraryCommand::Trash => encode_reply(library.trash()?),
        LibraryCommand::RestoreBook { content_hash } => encode_reply(library.restore_book(&content_hash)?),
        LibraryCommand::RestoreDirectory { directory_id, parent_id } => encode_reply(library.restore_directory(&directory_id, parent_id.as_ref())?),
        LibraryCommand::PurgeBook { content_hash } => encode_reply(library.purge_book(&content_hash)?),
        LibraryCommand::PurgeDirectory { directory_id } => encode_reply(library.purge_directory(&directory_id)?),
        LibraryCommand::EmptyTrash => encode_reply(library.empty_trash()?),
        // The new folder identity stays server-side; callers refresh instead.
        LibraryCommand::CreateDirectory { parent_id, name } => encode_reply(library.create_directory(&parent_id, &name).map(|_| ())?),
        LibraryCommand::RenameDirectory { directory_id, name } => encode_reply(library.rename_directory(&directory_id, &name)?),
        LibraryCommand::MoveDirectory { directory_id, parent_id } => encode_reply(library.move_directory(&directory_id, &parent_id)?),
        LibraryCommand::CopyDirectory { directory_id, parent_id } => encode_reply(library.copy_directory(&directory_id, &parent_id)?),
        LibraryCommand::RestoreBookPlacement { content_hash, directory_id } => encode_reply(library.restore_book_placement(&content_hash, &directory_id)?),
        LibraryCommand::CopyBookToDirectory { content_hash, source_id, destination_id } => encode_reply(library.copy_book_to_directory(&content_hash, &source_id, &destination_id)?),
        LibraryCommand::MoveBookToDirectory { content_hash, source_id, destination_id } => encode_reply(library.move_book_to_directory(&content_hash, &source_id, &destination_id)?),
        LibraryCommand::RemoveBookFromDirectory { content_hash, source_id } => encode_reply(library.remove_book_from_directory(&content_hash, &source_id)?),
        LibraryCommand::MoveBookToTrash { content_hash } => encode_reply(library.move_book_to_trash(&content_hash)?),
        LibraryCommand::MoveDirectoryToTrash { directory_id } => encode_reply(library.move_directory_to_trash(&directory_id)?),
        LibraryCommand::ImportBookBytes { parent_id, file_name, bytes } => encode_reply(library.import_book_bytes(&parent_id, file_name, bytes).await?),
        // Staging registers the activity first so a failed preparation cannot
        // leave the import counted as running.
        LibraryCommand::PrepareStagedDirectoryImport { activity, job, parent_id, name, directories } => {
            library.begin_directory_import(activity);
            let prepared = library.prepare_directory_import_job(&parent_id, name, directories, Some(job)).await;
            if prepared.is_err() {
                library.finish_directory_import(activity);
            }
            encode_reply(prepared?)
        }
        LibraryCommand::FinishDirectoryImport { activity } => encode_reply(library.finish_directory_import(activity)),
        LibraryCommand::DirectoryImportProgress => encode_reply(library.directory_import_progress()),
        LibraryCommand::AdvanceDirectoryImport { activity, total, succeeded, failed } => encode_reply(library.advance_directory_import(activity, total, succeeded, failed)),
        LibraryCommand::FolderContents { query } => encode_reply(library.folder_browse_data(&query, library.downloaded_books_only()?)?),
        LibraryCommand::SubjectContents { query } => encode_reply(library.subject_browse_data(&query, library.downloaded_books_only()?)?),
        LibraryCommand::BookDetail { content_hash } => encode_reply(library.book_detail(content_hash)?),
        LibraryCommand::Authors { sort } => encode_reply(library.authors(sort, library.downloaded_books_only()?)?),
        LibraryCommand::BookFormat { content_hash } => encode_reply(library.book_format(content_hash)?),
        LibraryCommand::AudiobookPlaybackMetadata { content_hash } => encode_reply(library.audiobook_playback_metadata(content_hash).await?),
        LibraryCommand::ReadingPosition { content_hash } => encode_reply(library.reading_position(content_hash)?),
        LibraryCommand::PdfReadingPosition { content_hash } => encode_reply(library.pdf_reading_position(content_hash)?),
        LibraryCommand::UpdateReadingPosition { content_hash, position, progress, entry } => encode_reply(library.update_reading_position(content_hash, &position, progress, entry.as_deref())?),
        LibraryCommand::Annotations { content_hash } => encode_reply(library.annotations(content_hash)?),
        LibraryCommand::UpsertAnnotation { annotation } => encode_reply(library.upsert_annotation(&annotation)?),
        LibraryCommand::DeleteAnnotation { annotation_id, modified_at } => encode_reply(library.delete_annotation(&annotation_id, modified_at)?),
        LibraryCommand::EnrichmentStatus => encode_reply(library.enrichment_status().await?),
    }
}

#[cfg(target_arch = "wasm32")]
async fn dispatch_host_command(command: HostLibraryCommand, library: &LibrarySession) -> Result<WorkerPayload, String> {
    command.dispatch_library(library).await
}

#[cfg(not(target_arch = "wasm32"))]
async fn dispatch_host_command(command: HostLibraryCommand, _: &LibrarySession) -> Result<WorkerPayload, String> {
    match command {}
}
