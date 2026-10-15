use super::LibraryClient;
use crate::ContentHash;
use crate::library::{LibraryCommand, PendingCommand, ReadyCommand};
use library_database::torrents::BookTorrent;
use library_model::{TorrentInput, TorrentStatus};

impl LibraryClient {
    pub async fn add_torrent(&self, input: TorrentInput) -> Result<String, String> {
        self.request(LibraryCommand::Pending(PendingCommand::AddTorrent { input })).await
    }

    pub async fn add_torrent_request(&self, input: TorrentInput, request: String) -> Result<String, String> {
        self.request(LibraryCommand::Pending(PendingCommand::AddTorrentRequest { input, request })).await
    }

    pub async fn prepare_torrent_request(&self, request: String) -> Result<(), String> {
        self.request(LibraryCommand::Ready(ReadyCommand::PrepareTorrentRequest { request })).await
    }

    pub async fn cancel_torrent_request(&self, request: String) -> Result<Option<String>, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::CancelTorrentRequest { request })).await
    }

    pub async fn cancel_torrent(&self, info_hash: String) -> Result<(), String> {
        self.request(LibraryCommand::Pending(PendingCommand::CancelTorrent { info_hash })).await
    }

    pub async fn torrent_transfers(&self) -> Result<Vec<TorrentStatus>, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::TorrentTransfers)).await
    }

    pub async fn book_torrents(&self, content_hash: ContentHash) -> Result<Vec<BookTorrent>, String> {
        self.request(LibraryCommand::Ready(ReadyCommand::BookTorrents { content_hash })).await
    }

    pub async fn keep_torrent_seeding(&self, info_hash: String, keep: bool) -> Result<(), String> {
        self.request(LibraryCommand::Ready(ReadyCommand::KeepTorrentSeeding { info_hash, keep })).await
    }

    /// Forget the transfer and associations, retaining all downloaded files.
    pub async fn forget_torrent(&self, info_hash: String) -> Result<(), String> {
        self.request(LibraryCommand::Ready(ReadyCommand::ForgetTorrent { info_hash })).await
    }
}
