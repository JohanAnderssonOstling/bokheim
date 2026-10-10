//! Native book readers, filesystem availability, and playback resources.

use super::*;
use crate::DownloadState;
use crate::library::DocumentBook;
use crate::library::ResolvedBook;
use crate::library::remote_access::read_remote_file;
use book_access::{FetchFuture, RemoteFileError};
pub(crate) type Opened = ResolvedBook;

pub(super) async fn local(library: &LibrarySession, hash: ContentHash, info: &BookInfo) -> Result<Option<Opened>, BackendError> {
    if !matches!(library.download_state(hash)?, DownloadState::Downloaded) {
        return Ok(None);
    }
    (async |library: &LibrarySession, content_hash: ContentHash, info: &BookInfo| -> Result<Opened, BackendError> {
        if info.format == BookFormat::Mp3Folder {
            return library.prepare_audiobook_track(content_hash, 0).await;
        }
        let mut path = info.path.clone().ok_or_else(|| BackendError::message("book placement not found"))?;
        let format = info.format;
        if !matches!(format, BookFormat::Epub | BookFormat::Pdf | BookFormat::Mobi | BookFormat::M4b) {
            return Err(format!("the {} format is no longer supported", format.canonical_extension()).into());
        }
        if matches!(format, BookFormat::Epub | BookFormat::Mobi) {
            let mut source = library.assets.open_async_book_at_path(path.clone()).await.map_err(BackendError::operation)?;
            if source.is_none() {
                if let Some(fresh) = refresh_local_path(library, content_hash).await? {
                    path = fresh;
                    source = library.assets.open_async_book_at_path(path.clone()).await.map_err(BackendError::operation)?;
                }
            }
            let source = source.ok_or_else(|| "book content is not downloaded")?;
            return Ok(ResolvedBook::Document(DocumentBook::from_async_source(path.to_string(), format, source)));
        }
        #[cfg(target_os = "android")]
        let playback_file = if format == BookFormat::M4b { library.assets.open_playback_file(&path)? } else { None };
        #[cfg(target_os = "android")]
        let reader = if let Some(file) = &playback_file { Some(Box::new(file.try_clone()?) as crate::BoxedBookReader) } else { library.assets.open_book_at_path(path.clone()).await? };
        #[cfg(not(target_os = "android"))]
        let mut reader = library.assets.open_book_at_path(path.clone()).await.map_err(BackendError::operation)?;
        #[cfg(not(target_os = "android"))]
        if reader.is_none() {
            if let Some(fresh) = refresh_local_path(library, content_hash).await? {
                path = fresh;
                reader = library.assets.open_book_at_path(path.clone()).await.map_err(BackendError::operation)?;
            }
        }
        let reader = reader.ok_or_else(|| "book content is not downloaded")?;
        if format == BookFormat::M4b {
            Ok(ResolvedBook::Audiobook(crate::library::AudiobookBook {
                format,
                path: path.to_string(),
                reader,
                stream_control: None,
                #[cfg(target_os = "android")]
                playback_file,
            }))
        } else {
            let metadata = if format == BookFormat::Pdf { library.db.local_pdf_metadata(&content_hash, path.as_str())? } else { None };
            let mut document = DocumentBook::new(path.to_string(), format, reader);
            document.pdf_metadata = metadata;
            Ok(ResolvedBook::Document(document))
        }
    })(library, hash, info)
    .await
    .map(Some)
}

// A placement move can land between reading `info.path` and opening it.
// Drain this hash's pending work and retry once against the fresh path.
async fn refresh_local_path(library: &LibrarySession, content_hash: ContentHash) -> Result<Option<library_model::RelativeBookPath>, BackendError> {
    let _ = library_placement::run_file_jobs_for(&library.assets, &library.db, &content_hash).await;
    Ok(book_info(library, content_hash).await?.path)
}

pub(super) async fn remote(library: &LibrarySession, hash: ContentHash, info: &BookInfo) -> Result<Opened, BackendError> {
    if info.format == BookFormat::Mp3Folder {
        return library.prepare_audiobook_track(hash, 0).await;
    }
    let path = info.path.clone().ok_or_else(|| BackendError::message("book placement not found"))?;
    let RemoteDocument { source, prefix, pdf_metadata } = remote_document(library, hash).await?;
    let length = source.length;
    let account = library.sync.clone().account().ok_or_else(|| BackendError::message("library account access is unavailable"))?;
    let library_id = *library.id();
    let client = library.sync.http_client.clone();
    if info.format == BookFormat::M4b {
        let fetch = move |offset, length| {
            let account = account.clone();
            let source = source.clone();
            let client = client.clone();
            Box::pin(async move { read_remote_file(&client, &account, library_id, &source, offset, length).await }) as FetchFuture<'static, Result<Vec<u8>, RemoteFileError>>
        };
        let (reader, stream_control) = book_access::remote_file::native_audio_reader(length, prefix, fetch).map_err(BackendError::operation)?;
        return Ok(ResolvedBook::Audiobook(AudiobookBook {
            format: info.format,
            path: path.to_string(),
            reader,
            stream_control: Some(stream_control),
            #[cfg(target_os = "android")]
            playback_file: None,
        }));
    }
    let fetch = move |offset, length| {
        let account = account.clone();
        let source = source.clone();
        let client = client.clone();
        Box::pin(async move { read_remote_file(&client, &account, library_id, &source, offset, length).await }) as FetchFuture<'static, Result<Vec<u8>, RemoteFileError>>
    };
    if matches!(info.format, BookFormat::Epub | BookFormat::Mobi) {
        let source = book_access::AsyncBookSource::remote(length, prefix, fetch).map_err(BackendError::operation)?;
        return Ok(ResolvedBook::Document(DocumentBook::from_async_source(path.to_string(), info.format, source)));
    }
    let reader = book_access::remote_file::native_reader(length, prefix, fetch).map_err(BackendError::operation)?;
    let mut document = ResolvedBook::Document(DocumentBook::new(path.to_string(), info.format, reader));
    if let ResolvedBook::Document(book) = &mut document {
        book.pdf_metadata = pdf_metadata;
    }
    Ok(document)
}

impl LibrarySession {
    pub(crate) async fn prepare_audiobook_track(&self, hash: ContentHash, index: usize) -> Result<ResolvedBook, BackendError> {
        let info = book_info(self, hash).await?;
        if info.format != BookFormat::Mp3Folder {
            return Err(BackendError::message("book is not an MP3 folder"));
        }
        let track = self.db.audiobook_tracks(&hash)?.into_iter().nth(index).ok_or_else(|| BackendError::message("audiobook track is unavailable"))?;
        if !track.valid() {
            return Err(BackendError::message("audiobook track index is invalid"));
        }
        let mut path = info.path.ok_or_else(|| BackendError::message("audiobook placement is unavailable"))?;
        if matches!(self.download_state(hash)?, DownloadState::Downloaded) {
            let mut reader = self.assets.open_book_at_path(path.join(&track.name).map_err(BackendError::operation)?).await.map_err(BackendError::operation)?;
            if reader.is_none() {
                if let Some(fresh) = refresh_local_path(self, hash).await? {
                    path = fresh;
                    reader = self.assets.open_book_at_path(path.join(&track.name).map_err(BackendError::operation)?).await.map_err(BackendError::operation)?;
                }
            }
            if let Some(reader) = reader {
                return Ok(ResolvedBook::Audiobook(AudiobookBook {
                    format: BookFormat::Mp3Folder,
                    path: path.to_string(),
                    reader,
                    stream_control: None,
                    #[cfg(target_os = "android")]
                    playback_file: None,
                }));
            }
        }
        let (source, _) = self.describe_remote_file(hash).await.map_err(BackendError::operation)?;
        if track.archive_hash != Some(source.hash) {
            return Err(BackendError::message("audiobook track index does not match the remote archive; sync the book metadata and retry"));
        }
        if track.offset.checked_add(track.length).is_none_or(|end| end > source.length) {
            return Err(BackendError::message("audiobook track exceeds remote archive"));
        }
        let account = self.sync.clone().account().ok_or_else(|| BackendError::message("library account access is unavailable"))?;
        let library_id = *self.id();
        let client = self.sync.http_client.clone();
        let prefix = read_remote_file(&client, &account, library_id, &source, track.offset, track.length.min(64 * 1024) as usize).await.map_err(BackendError::operation)?;
        let base = track.offset;
        let length = track.length;
        let fetch = move |offset: u64, count: usize| {
            let account = account.clone();
            let source = source.clone();
            let client = client.clone();
            Box::pin(async move {
                if offset.checked_add(count as u64).is_none_or(|end| end > length) {
                    return Err(RemoteFileError::Failed("audiobook track range is invalid".into()));
                }
                read_remote_file(&client, &account, library_id, &source, base + offset, count).await
            }) as FetchFuture<'static, Result<Vec<u8>, RemoteFileError>>
        };
        let (reader, stream_control) = book_access::remote_file::native_audio_reader(length, prefix, fetch).map_err(BackendError::operation)?;
        Ok(ResolvedBook::Audiobook(AudiobookBook {
            format: BookFormat::Mp3Folder,
            path: path.to_string(),
            reader,
            stream_control: Some(stream_control),
            #[cfg(target_os = "android")]
            playback_file: None,
        }))
    }
}

pub struct AudiobookBook {
    pub format: BookFormat,
    pub path: String,
    pub reader: crate::library::BoxedBookReader,
    pub stream_control: Option<crate::library::AudioStreamControl>,
    #[cfg(target_os = "android")]
    pub playback_file: Option<std::fs::File>,
}
