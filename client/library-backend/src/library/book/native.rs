//! Native book readers, filesystem availability, and playback resources.
use super::*;
use crate::library::ResolvedBook;
#[cfg(target_os = "android")]
use crate::BlobKind;
pub(crate) type Opened = ResolvedBook;

pub(super) fn supports_streaming(format: BookFormat) -> bool {
    matches!(format, BookFormat::Epub | BookFormat::Pdf | BookFormat::M4b | BookFormat::Mobi)
}
pub(super) async fn local(library: &LibrarySession, hash: ContentHash, info: &BookInfo) -> Result<Option<Opened>, BackendError> {
    if !matches!(library.download_state(hash)?, DownloadState::Downloaded) {
        return Ok(None);
    }
    resolve_local(library, hash, info).await.map(Some)
}

impl LibrarySession {
    pub(crate) async fn resolve_book_for_reader(&self, hash: ContentHash) -> Result<Opened, BackendError> {
        resolve_local(self, hash, &book_info(self, hash).await?).await
    }
}
// A placement move can land between reading `info.path` and opening it.
// Drain this hash's pending work and retry once against the fresh path.
async fn refresh_local_path(library: &LibrarySession, content_hash: ContentHash) -> Result<Option<String>, BackendError> {
    let _ = library.assets.run_file_jobs_for(&library.db, &content_hash).await;
    Ok(book_info(library, content_hash).await?.path)
}

async fn resolve_local(library: &LibrarySession, content_hash: ContentHash, info: &BookInfo) -> Result<Opened, BackendError> {
    let mut path = info.path.clone().ok_or_else(|| BackendError::message(format!("book placement not found: {content_hash}")))?;
    let format = info.format;
    if !matches!(format, BookFormat::Epub | BookFormat::Pdf | BookFormat::Mobi | BookFormat::M4b) {
        return Err(format!("the {} format is no longer supported", format.canonical_extension()).into());
    }
    if format == BookFormat::Pdf {
        let mut prepared = library.assets.prepare_pdf_at_path(path.clone()).await.map_err(BackendError::operation)?;
        if prepared.is_none() {
            if let Some(fresh) = refresh_local_path(library, content_hash).await? {
                path = fresh;
                prepared = library.assets.prepare_pdf_at_path(path.clone()).await.map_err(BackendError::operation)?;
            }
        }
        let (reader, fingerprint) = prepared.ok_or_else(|| format!("book content is not downloaded: {content_hash}"))?;
        let metadata = library.db.local_pdf_metadata(&content_hash, &fingerprint, &path)?;
        let mut document = crate::library::DocumentBook::new(path, format, reader);
        document.pdf_metadata = metadata;
        return Ok(ResolvedBook::Document(document));
    }
    #[cfg(target_os = "android")]
    let playback_file = if format == BookFormat::M4b { library.assets.open_playback_file(&content_hash)? } else { None };
    #[cfg(target_os = "android")]
    let reader = if let Some(file) = &playback_file { Some(Box::new(file.try_clone()?) as crate::BoxedBookReader) } else { library.assets.prepare_reader(BlobKind::Book, &content_hash).await? };
    #[cfg(not(target_os = "android"))]
    let mut reader = library.assets.prepare_book_at_path(path.clone()).await.map_err(BackendError::operation)?;
    #[cfg(not(target_os = "android"))]
    if reader.is_none() {
        if let Some(fresh) = refresh_local_path(library, content_hash).await? {
            path = fresh;
            reader = library.assets.prepare_book_at_path(path.clone()).await.map_err(BackendError::operation)?;
        }
    }
    let reader = reader.ok_or_else(|| format!("book content is not downloaded: {content_hash}"))?;
    if format == BookFormat::M4b {
        Ok(ResolvedBook::Audiobook(crate::library::AudiobookBook {
            path,
            reader,
            stream_control: None,
            #[cfg(target_os = "android")]
            playback_file,
        }))
    } else {
        Ok(ResolvedBook::Document(crate::library::DocumentBook::new(path, format, reader)))
    }
}

pub(super) async fn remote(library: &LibrarySession, hash: ContentHash, info: &BookInfo) -> Result<Opened, BackendError> {
    let path = info.path.clone().ok_or_else(|| BackendError::message(format!("book placement not found: {hash}")))?;
    let RemoteDocument { source, prefix, pdf_metadata } = remote_document(library, hash).await?;
    let length = source.length;
    let account = library.sync().and_then(|sync| sync.account()).ok_or_else(|| BackendError::message("library account access is unavailable"))?;
    let library_id = *library.id();
    if info.format == BookFormat::M4b {
        let fetch = move |offset, length| {
            let account = account.clone();
            let source = source.clone();
            Box::pin(async move { crate::library::remote_access::read_remote_file_typed(&reqwest::Client::new(), &account, library_id, &source, offset, length).await })
                as book_access::FetchFuture<'static, Result<Vec<u8>, book_access::remote_file::RemoteFileError>>
        };
        let (reader, stream_control) = book_access::remote_file::native_audio_reader(length, prefix, fetch).map_err(BackendError::operation)?;
        return Ok(ResolvedBook::Audiobook(AudiobookBook {
            path,
            reader,
            stream_control: Some(stream_control),
            #[cfg(target_os = "android")]
            playback_file: None,
        }));
    }
    let fetch = move |offset, length| {
        let account = account.clone();
        let source = source.clone();
        Box::pin(async move { crate::library::remote_access::read_remote_file_typed(&reqwest::Client::new(), &account, library_id, &source, offset, length).await.map_err(|error| error.to_string()) })
            as book_access::FetchFuture<'static, Result<Vec<u8>, String>>
    };
    let reader = book_access::remote_file::native_reader(length, prefix, fetch).map_err(BackendError::operation)?;
    let mut document = ResolvedBook::Document(crate::library::DocumentBook::new(path, info.format, reader));
    if let ResolvedBook::Document(book) = &mut document {
        book.pdf_metadata = pdf_metadata;
    }
    Ok(document)
}

pub struct AudiobookBook {
    pub path: String,
    pub reader: crate::library::BoxedBookReader,
    pub stream_control: Option<crate::library::AudioStreamControl>,
    #[cfg(target_os = "android")]
    pub playback_file: Option<std::fs::File>,
}
