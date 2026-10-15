//! Host resources for copying verified book bytes between library actors.
use super::LibrarySession;
use super::remote_access::read_remote_file;
use book_access::{BoxedBookReader, FetchFuture, RemoteFileError};
use client_runtime::BackendError;
use library_database::transfer::BookTransferState;
use sync_common::ContentHash;

pub struct TransferResource {
    pub reader: BoxedBookReader,
    pub state: BookTransferState,
}
impl LibrarySession {
    pub(crate) async fn export_transfer_book(&self, hash: ContentHash) -> Result<TransferResource, BackendError> {
        library_placement::run_file_jobs_for(&self.assets, &self.db, &hash).await.map_err(BackendError::operation)?;
        let state = self.db.book_transfer_state(hash)?;
        let paths = self.db.book_paths(&hash)?;
        if let Some(book) = self.assets.verified_book_reader(hash, paths).await.map_err(BackendError::operation)? {
            return Ok(TransferResource { reader: Box::new(book), state });
        }
        // Read the complete original asset, including an MP3 folder's archive.
        let (source, prefix) = self.describe_remote_file(hash).await.map_err(BackendError::operation)?;
        let length = source.length;
        let account = self.sync.clone().account().ok_or_else(|| BackendError::message("library account access is unavailable"))?;
        let library_id = self.id;
        let client = self.sync.http_client.clone();
        let fetch = move |offset, length| {
            let (account, source, client) = (account.clone(), source.clone(), client.clone());
            Box::pin(async move { read_remote_file(&client, &account, library_id, &source, offset, length).await }) as FetchFuture<'static, Result<Vec<u8>, RemoteFileError>>
        };
        let reader = book_access::remote_file::native_reader(length, prefix, fetch).map_err(BackendError::operation)?;
        Ok(TransferResource { reader, state })
    }
}
