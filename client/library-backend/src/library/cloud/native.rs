//! Download publication into native library placements.

use super::*;
use book_access::{FetchFuture, RemoteFileError};
use cpu_host::ThumbnailBytes;
use library_database::NativeBookCompletion;
use std::path::PathBuf;

impl LibrarySession {
    pub(super) async fn inspect_cloud_file(&self, connection: ConnectedDrive, file: ProviderFile, name: String, format: BookFormat) -> Result<book_inspection::InspectedBook, BackendError> {
        let id = file.identity.book_id();
        let reader = cloud_reader(connection, file).await?;
        client_platform_runtime::executor::run_blocking(move || book_inspection::inspect_book_reader_with_hash(&PathBuf::from(name), format, reader, Some(id)).map_err(BackendError::operation)).await.map_err(BackendError::operation)?
    }

    pub(super) async fn generate_cloud_thumbnail(&self, connection: ConnectedDrive, file: ProviderFile) -> Result<Option<ThumbnailBytes>, BackendError> {
        let Some(format) = self.db.thumbnail_book_format(file.identity.book_id())? else { return Ok(None) };
        let reader = cloud_reader(connection, file).await?;
        self.cpu.generate_thumbnail(format.canonical_extension().into(), cpu_host::as_cpu_reader(reader)).await
    }

    pub(super) async fn download_cloud_book(&self, connection: &ConnectedDrive, id: BookId) -> Result<(), BackendError> {
        let file = self.db.provider_file(&id)?.ok_or_else(|| BackendError::message("cloud book is unavailable"))?;
        let snapshot = self.db.transfer_snapshot(&[id])?;
        let Some(book) = snapshot.placements.iter().find_map(|entry| entry.book.clone()) else {
            self.db.cancel_download(&id)?;
            return Ok(());
        };
        self.assets.check_write_capacity(file.size_bytes).map_err(BackendError::operation)?;
        self.assets.ensure_placement_parent(&book.relative_path).map_err(BackendError::operation)?;
        let mut writer = self.assets.begin_book_write(&book.relative_path, &id).await.map_err(BackendError::operation)?;
        let mut completed = 0;
        while completed < file.size_bytes {
            let length = (file.size_bytes - completed).min(cloud_storage::MAX_RANGE_BYTES as u64) as usize;
            let bytes = connection.read_range(&file, completed, length).await.map_err(BackendError::operation)?;
            writer.write_all(&bytes).await.map_err(BackendError::operation)?;
            completed += bytes.len() as u64;
            self.emit(LibraryEvent::DownloadStatusChanged { content_hash: id, state: DownloadState::Downloading(DownloadProgress::from_ratio(completed, file.size_bytes)) });
        }
        if self.db.provider_file(&id)?.is_none_or(|current| current.revision != file.revision) {
            return Err(BackendError::message("cloud file changed during download"));
        }
        let writer = writer.verify_provider(file.size_bytes, file.content_hash).await.map_err(BackendError::operation)?;
        let staged = writer.finish_book().await.map_err(BackendError::operation)?;
        let publication = staged.publish_snapshot_with_track_titles(&self.assets, &book.relative_path, &book.occupied_names, book.format == BookFormat::Mp3Folder, &book.audiobook_track_titles).map_err(BackendError::operation)?;
        self.db.complete_downloaded_book(NativeBookCompletion { snapshot: book, publication, size_bytes: completed, provider_file: Some(file) }, self.assets.thumbnail_set_exists(&id).map_err(BackendError::operation)?)?;
        self.emit(LibraryEvent::DownloadStatusChanged { content_hash: id, state: self.persistent_download_state(id)? });
        self.events.notify_contents_changed();
        Ok(())
    }
}

async fn cloud_reader(connection: ConnectedDrive, file: ProviderFile) -> Result<book_access::BoxedBookReader, BackendError> {
    let length = file.size_bytes;
    let prefix = connection.read_range(&file, 0, length.min(64 * 1024) as usize).await.map_err(BackendError::operation)?;
    let fetch = move |offset, length| {
        let connection = connection.clone();
        let file = file.clone();
        Box::pin(async move { connection.read_range(&file, offset, length).await.map_err(|error| RemoteFileError::Failed(error.to_string())) }) as FetchFuture<'static, Result<Vec<u8>, RemoteFileError>>
    };
    book_access::remote_file::native_reader(length, prefix, fetch).map_err(BackendError::operation)
}
