//! Browser ranges feed CPU workers and stream offline copies into OPFS.

use super::*;
use book_access::AsyncBookSource;
use client_platform_web::{cpu_host::run_book_operation_with_source, transport::range::Server};
use cpu_host::ThumbnailBytes;
use futures_util::io::{AsyncReadExt, AsyncSeekExt};
use std::{io::SeekFrom, rc::Rc};
use tokio::sync::Mutex;

impl LibrarySession {
    pub(super) async fn inspect_cloud_file(&self, connection: ConnectedDrive, file: ProviderFile, name: String, format: BookFormat) -> Result<book_inspection::InspectedBook, BackendError> {
        let id = file.identity.book_id();
        let source = browser_cloud_source(connection, file)?;
        run_book_operation_with_source(cpu_host::Inspect { source_name: name, format, content_hash: Some(id) }, source.source()).await.map_err(BackendError::message)
    }

    pub(super) async fn generate_cloud_thumbnail(&self, connection: ConnectedDrive, file: ProviderFile) -> Result<Option<ThumbnailBytes>, BackendError> {
        let Some(format) = self.db.thumbnail_book_format(file.identity.book_id())? else { return Ok(None) };
        let source = browser_cloud_source(connection, file)?;
        run_book_operation_with_source(cpu_host::Thumbnail { extension: format.canonical_extension().into() }, source.source()).await.map_err(BackendError::message)
    }

    pub(super) async fn download_cloud_book(&self, connection: &ConnectedDrive, id: BookId) -> Result<(), BackendError> {
        let Some(snapshot) = self.db.provider_download_snapshot(id)? else {
            self.db.complete_download(&id)?;
            return Ok(());
        };
        let _lease = self.assets.lease_book(&id).await.map_err(BackendError::operation)?;
        let file = &snapshot.file;
        let mut writer = self.assets.begin_provider_write(file).await.map_err(BackendError::operation)?;
        let mut completed = 0;
        while completed < file.size_bytes {
            if self.db.provider_download_snapshot(id)?.as_ref() != Some(&snapshot) {
                return Ok(());
            }
            let length = (file.size_bytes - completed).min(cloud_storage::MAX_RANGE_BYTES as u64) as usize;
            let bytes = connection.read_range(file, completed, length).await.map_err(BackendError::operation)?;
            writer.write_all(&bytes).await.map_err(BackendError::operation)?;
            completed += bytes.len() as u64;
            self.emit(LibraryEvent::DownloadStatusChanged { content_hash: id, state: DownloadState::Downloading(DownloadProgress::from_ratio(completed, file.size_bytes)) });
        }
        let publication = writer.publish().await.map_err(BackendError::operation)?;
        match self.db.complete_provider_download(&snapshot) {
            Ok(true) => {}
            Ok(false) => publication.discard().map_err(BackendError::message)?,
            Err(error) => {
                publication.discard().map_err(BackendError::message)?;
                return Err(error.into());
            }
        }
        self.emit(LibraryEvent::DownloadStatusChanged { content_hash: id, state: self.persistent_download_state(id)? });
        self.events.notify_contents_changed();
        Ok(())
    }
}

/// A bounded cache prevents parsers' tiny synchronous reads from becoming
/// individual Drive requests. Only the CPU worker blocks on the bridge;
/// authenticated HTTP stays asynchronous in the owning library worker.
fn browser_cloud_source(connection: ConnectedDrive, file: ProviderFile) -> Result<Server, BackendError> {
    let length = file.size_bytes;
    let source = AsyncBookSource::browser(length, Vec::new(), move |offset, length| {
        let connection = connection.clone();
        let file = file.clone();
        Box::pin(async move { connection.read_range(&file, offset, length).await.map_err(|error| error.to_string()) })
    })
    .map_err(BackendError::operation)?;
    let reader = Rc::new(Mutex::new(source.reader()));
    Server::asynchronous(length, move |offset, count| {
        let reader = reader.clone();
        async move {
            let mut reader = reader.lock().await;
            reader.seek(SeekFrom::Start(offset)).await.map_err(|error| error.to_string())?;
            let mut bytes = vec![0; count.min(256 * 1024)];
            reader.read_exact(&mut bytes).await.map_err(|error| error.to_string())?;
            Ok(bytes)
        }
    })
    .map_err(BackendError::message)
}
