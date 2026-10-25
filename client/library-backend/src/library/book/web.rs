//! Browser book capabilities and media URLs with their owning leases.
use super::browser::BookLease;
use super::*;
use crate::book::{BookSource, ResolvedBookData};
pub(crate) type Opened = (ResolvedBookData, BookLease);

pub(super) async fn local(library: &LibrarySession, hash: BookId, info: &BookInfo) -> Result<Option<Opened>, BackendError> {
    let Some(file) = library.assets.playback_reader(&hash).await.map_err(BackendError::operation)? else { return Ok(None) };
    if matches!(info.format, BookFormat::M4b | BookFormat::Mp3Folder) {
        return audio(library, hash, Some(file)).await.map(Some);
    }
    let pdf_metadata = match file.content_hash().map_err(BackendError::operation)? {
        Some(_content_hash) => pdf_metadata(library, hash).await?,
        None => None,
    };
    use crate::library::book::browser::SOURCES;
    let (source, lease) = client_platform_web::range_reader::register(file).map_err(BackendError::operation)?;
    let capability = source.capability.clone();
    SOURCES.with(|sources| sources.borrow_mut().insert(capability.clone(), (*library.id(), lease)));
    let source = BookSource::Local { capability: source.capability, length: source.length };
    let lease = BookLease::Local(capability);
    Ok(Some((ResolvedBookData { pdf_metadata, format: info.format, source }, lease)))
}
pub(super) async fn remote(library: &LibrarySession, hash: BookId, info: &BookInfo) -> Result<Opened, BackendError> {
    if matches!(info.format, BookFormat::M4b | BookFormat::Mp3Folder) {
        return audio(library, hash, None).await;
    }
    let RemoteDocument { source, prefix, pdf_metadata } = remote_document(library, hash).await?;
    Ok((ResolvedBookData { pdf_metadata, format: info.format, source: BookSource::Remote { source, prefix } }, BookLease::None))
}
async fn audio(library: &LibrarySession, hash: BookId, file: Option<client_platform_web::web_storage::FileReader>) -> Result<Opened, BackendError> {
    let (source, lease) = crate::library::browser_audiobook::prepare_with_file(library, hash, file).await.map_err(BackendError::operation)?;
    let format = library.db.book_format(&hash).map_err(BackendError::operation)?.unwrap_or(BookFormat::M4b);
    Ok((ResolvedBookData { pdf_metadata: None, format, source: BookSource::Audiobook(source) }, BookLease::Audiobook(lease)))
}

pub type AudiobookBook = crate::library::browser_audiobook::BrowserAudiobook;
