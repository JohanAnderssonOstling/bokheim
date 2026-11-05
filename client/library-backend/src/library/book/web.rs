//! Browser book capabilities and media URLs with their owning leases.
use super::browser::BookLease;
use super::*;
use crate::library::{browser_audiobook, DirectPlayback};
use book_access::RemoteFileError;
use client_platform_web::web_storage::FileReader;
use library_model::PlaybackMetadata;
use remote_book_access::direct_audio_source;
use std::future::Future;
pub(crate) type Opened = (ResolvedBookData, BookLease);

pub(super) async fn local(library: &LibraryResources, storage: &LibraryState, hash: BookId, info: &LocalBookInfo) -> Result<Option<Opened>, BackendError> {
    let Some(file) = storage.assets().playback_reader(&hash).await.map_err(BackendError::operation)? else { return Ok(None) };
    if matches!(info.format, BookFormat::M4b | BookFormat::Mp3Folder) {
        let metadata = storage.database().audiobook_playback_metadata(&hash).await?;
        return audio(metadata, Some(file), direct_audio_source(library.sync.http_client(), &library.account, library.id, hash)).await.map(Some);
    }
    let pdf_metadata = match file.book_id().await.map_err(BackendError::operation)? {
        Some(_book_id) => storage.database().pdf_metadata(&hash).await?,
        None => None,
    };
    use crate::library::book::browser::SOURCES;
    let (source, lease) = client_platform_runtime::executor::run_blocking(move || client_platform_web::range_reader::register(file)).await.map_err(BackendError::operation)?.map_err(BackendError::operation)?;
    let capability = source.capability.clone();
    SOURCES.lock().expect("browser book sources").insert(capability.clone(), (library.id, lease));
    let source = BookSource::Local { capability: source.capability, length: source.length };
    let lease = BookLease::Local(capability);
    Ok(Some((ResolvedBookData { pdf_metadata, format: info.format, source }, lease)))
}
pub(super) async fn remote(library: &LibraryResources, storage: &LibraryState, hash: BookId, info: &LocalBookInfo) -> Result<Opened, BackendError> {
    if matches!(info.format, BookFormat::M4b | BookFormat::Mp3Folder) {
        let metadata = storage.database().audiobook_playback_metadata(&hash).await?;
        return audio(metadata, None, direct_audio_source(library.sync.http_client(), &library.account, library.id, hash)).await;
    }
    Ok((library.remote_book_source(storage, hash, info.format).await?, BookLease::None))
}
async fn audio(metadata: Option<PlaybackMetadata>, file: Option<FileReader>, remote: impl Future<Output = Result<DirectPlayback, RemoteFileError>>) -> Result<Opened, BackendError> {
    let (source, lease) = browser_audiobook::prepare(metadata, file, remote).await?;
    let format = source.metadata.format;
    Ok((ResolvedBookData { pdf_metadata: None, format, source: BookSource::Audiobook(source) }, BookLease::Audiobook(lease)))
}

pub type AudiobookBook = browser_audiobook::BrowserAudiobook;
