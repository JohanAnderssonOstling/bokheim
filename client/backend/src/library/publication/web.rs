//! Browser publication capabilities and media URLs with their owning leases.
use super::*;
use crate::library::browser_publication::PublicationLease;
use crate::runtime::{PublicationSource, ResolvedBookData};
pub(crate) type Opened = (ResolvedBookData, PublicationLease);

pub(super) fn supports_streaming(format: BookFormat) -> bool {
    matches!(format, BookFormat::Epub | BookFormat::Pdf | BookFormat::M4b)
}
pub(super) async fn local(library: &LibrarySession, hash: ContentHash, info: &BookInfo) -> Result<Option<Opened>, BackendError> {
    let Some(file) = library.assets.playback_reader(&hash).await.map_err(BackendError::operation)? else { return Ok(None) };
    if info.format == BookFormat::M4b {
        return audio(library, hash, Some(file)).await.map(Some);
    }
    let pdf_metadata = match file.checksum().map_err(BackendError::operation)? {
        Some(checksum) => pdf_metadata(library, hash, checksum).await?,
        None => None,
    };
    let (source, lease) = crate::library::browser_publication::register(*library.id(), file).map_err(BackendError::operation)?;
    Ok(Some((ResolvedBookData { pdf_metadata, format: info.format, source }, lease)))
}
pub(super) async fn remote(library: &LibrarySession, hash: ContentHash, info: &BookInfo) -> Result<Opened, BackendError> {
    if info.format == BookFormat::M4b {
        return audio(library, hash, None).await;
    }
    let RemoteDocument { source, prefix, pdf_metadata } = remote_document(library, hash).await?;
    Ok((ResolvedBookData { pdf_metadata, format: info.format, source: PublicationSource::Remote { source, prefix } }, PublicationLease::None))
}
async fn audio(library: &LibrarySession, hash: ContentHash, file: Option<crate::platform::web_storage::FileReader>) -> Result<Opened, BackendError> {
    let (source, lease) = crate::library::browser_audiobook::prepare_with_file(library, hash, file).await.map_err(BackendError::operation)?;
    Ok((ResolvedBookData { pdf_metadata: None, format: BookFormat::M4b, source: PublicationSource::Audiobook(source) }, PublicationLease::Audiobook(lease)))
}

pub type AudiobookBook = crate::library::browser_audiobook::BrowserAudiobook;
