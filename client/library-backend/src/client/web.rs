use crate::library::{DirectPlayback, LibraryHandle, PlaybackSource, RemoteFile};
use crate::{BackendError, BookId, ResolvedBookData};

impl crate::library::web_reader::BookTransport for LibraryHandle {
    async fn open_remote_audio(&self, book_id: BookId) -> Result<Option<PlaybackSource>, BackendError> {
        LibraryHandle::open_remote_audio(self, book_id).await
    }

    async fn renew_remote_audio(&self, book_id: BookId) -> Result<DirectPlayback, BackendError> {
        LibraryHandle::renew_remote_audio(self, book_id).await
    }

    async fn read_book_range(&self, capability: String, offset: u64, length: usize) -> Result<Vec<u8>, BackendError> {
        LibraryHandle::read_book_range(self, capability, offset, length).await
    }

    async fn read_file_range(&self, source: RemoteFile, offset: u64, length: usize) -> Result<Vec<u8>, BackendError> {
        LibraryHandle::read_file_range(self, source, offset, length).await
    }

    async fn read_file_ranges(&self, source: RemoteFile, ranges: Vec<(u64, usize)>) -> Result<Vec<u8>, BackendError> {
        LibraryHandle::read_file_ranges(self, source, ranges).await
    }

    async fn subscribe_book(&self, book_id: BookId) -> Result<(async_channel::Receiver<()>, ResolvedBookData), crate::BackendError> {
        let (book, lease) = LibraryHandle::subscribe_book(self, book_id).await?;
        let (sender, receiver) = async_channel::bounded(1);
        crate::executor::spawn_detached(async move {
            let _lease = lease;
            sender.closed().await;
        });
        Ok((receiver, book))
    }
}
