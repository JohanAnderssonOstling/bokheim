use crate::library::{DirectPlayback, LibraryCommand, PendingCommand, PlaybackSource, ReadyCommand, RemoteFile};
use crate::{BackendError, BookId, ResolvedBookData};

impl crate::library::web_reader::BookTransport for super::LibraryClient {
    async fn open_remote_audio(&self, book_id: BookId) -> Result<Option<PlaybackSource>, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::OpenRemoteAudio { book_id })).await
    }

    async fn renew_remote_audio(&self, book_id: BookId) -> Result<DirectPlayback, BackendError> {
        self.request(LibraryCommand::Pending(PendingCommand::RenewRemoteAudio { book_id })).await
    }

    async fn read_book_range(&self, capability: String, offset: u64, length: usize) -> Result<Vec<u8>, BackendError> {
        let bytes: serde_bytes::ByteBuf = self.request(LibraryCommand::Ready(ReadyCommand::ReadBookRange { capability, offset, length })).await?;
        Ok(bytes.into_vec())
    }

    async fn read_file_range(&self, source: RemoteFile, offset: u64, length: usize) -> Result<Vec<u8>, BackendError> {
        let bytes: serde_bytes::ByteBuf = self.request(LibraryCommand::Pending(PendingCommand::ReadFileRange { source, offset, length })).await?;
        Ok(bytes.into_vec())
    }

    async fn read_file_ranges(&self, source: RemoteFile, ranges: Vec<(u64, usize)>) -> Result<Vec<u8>, BackendError> {
        let bytes: serde_bytes::ByteBuf = self.request(LibraryCommand::Pending(PendingCommand::ReadFileRanges { source, ranges })).await?;
        Ok(bytes.into_vec())
    }

    async fn read_file_bundle(&self, source: RemoteFile, ranges: Vec<(u64, usize)>) -> Result<Vec<u8>, BackendError> {
        super::LibraryClient::read_file_bundle(self, source, ranges).await
    }

    async fn subscribe_book(&self, book_id: BookId) -> Result<(async_channel::Receiver<()>, ResolvedBookData), crate::BackendError> {
        self.transport.subscribe_book(book_id).await
    }
}
