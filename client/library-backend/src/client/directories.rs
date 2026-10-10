use super::LibraryClient;
use crate::library::{LibraryCommand, ReadyCommand};
use crate::{ContentHash, DirId};

impl LibraryClient {
    pub async fn restore_directory(&self, directory_id: DirId, parent_id: Option<DirId>) -> Result<String, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::RestoreDirectory { directory_id, parent_id })).await
    }

    pub async fn folder_destinations(&self) -> Result<Vec<library_model::LibraryFolderDestination>, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::FolderDestinations)).await
    }

    pub async fn purge_directory(&self, directory_id: DirId) -> Result<(), String> {
        self.request(LibraryCommand::Ready(ReadyCommand::PurgeDirectory { directory_id })).await
    }
    pub async fn empty_trash(&self) -> Result<(), String> {
        self.request(LibraryCommand::Ready(ReadyCommand::EmptyTrash)).await
    }
    pub async fn create_directory(&self, parent_id: DirId, name: String) -> Result<(), String> {
        self.request(LibraryCommand::Ready(ReadyCommand::CreateDirectory { parent_id, name })).await
    }
    pub async fn rename_directory(&self, directory_id: DirId, name: String) -> Result<String, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::RenameDirectory { directory_id, name })).await
    }
    pub async fn move_directory(&self, directory_id: DirId, parent_id: DirId) -> Result<String, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::MoveDirectory { directory_id, parent_id })).await
    }
    pub async fn copy_directory(&self, directory_id: DirId, parent_id: DirId) -> Result<DirId, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::CopyDirectory { directory_id, parent_id })).await
    }
    pub async fn restore_book_placement(&self, content_hash: ContentHash, directory_id: DirId) -> Result<(), String> {
        self.request(LibraryCommand::Ready(ReadyCommand::RestoreBookPlacement { content_hash, directory_id })).await
    }
    pub async fn move_directory_to_trash(&self, directory_id: DirId) -> Result<(), String> {
        self.request(LibraryCommand::Ready(ReadyCommand::MoveDirectoryToTrash { directory_id })).await
    }
    /// Drops the downloaded copies under one folder while keeping every
    /// entry. Books the server does not hold keep their bytes. Returns how
    /// many copies were evicted.
    pub async fn evict_folder_downloads(&self, directory_id: DirId) -> Result<usize, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::EvictFolderDownloads { directory_id })).await
    }

    pub async fn copy_book_to_directory(&self, content_hash: ContentHash, source_id: Option<DirId>, destination_id: DirId) -> Result<bool, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::CopyBookToDirectory { content_hash, source_id, destination_id })).await
    }
    pub async fn move_book_to_directory(&self, content_hash: ContentHash, source_id: DirId, destination_id: DirId) -> Result<String, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::MoveBookToDirectory { content_hash, source_id, destination_id })).await
    }
    pub async fn remove_book_from_directory(&self, content_hash: ContentHash, source_id: DirId) -> Result<bool, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::RemoveBookFromDirectory { content_hash, source_id })).await
    }
}
