//! Local media retains a storage lease; remote media uses expiring direct URLs.

pub use crate::library::{PlaybackLocation, PlaybackSource};
use library_model::PlaybackMetadata;
use crate::BackendError;
use crate::{ContentHash, LibrarySession};
pub type PlaybackLease = client_platform_web::audiobook::PlaybackLease;
pub type BrowserAudiobook = client_platform_web::audiobook::BrowserAudiobook<PlaybackMetadata, crate::library::DirectPlayback>;

pub(crate) async fn prepare(library: &LibrarySession, hash: ContentHash) -> Result<(PlaybackSource, PlaybackLease), BackendError> {
    let file = library.assets.playback_reader(&hash).await.map_err(BackendError::operation)?;
    prepare_with_file(library, hash, file).await
}

/// Browser-host command used when a local audiobook is absent.  The format
/// check and storage probe remain inside the owning library module.
pub async fn open_remote_audio(library: &LibrarySession, hash: ContentHash) -> Result<Option<PlaybackSource>, BackendError> {
    let format = library.db.book_format(&hash).map_err(BackendError::operation)?;
    if !matches!(format, Some(book_model::BookFormat::M4b | book_model::BookFormat::Mp3Folder)) || library.assets.playback_reader(&hash).await.map_err(BackendError::operation)?.is_some() {
        return Ok(None);
    }
    let (source, _lease) = prepare(library, hash).await?;
    Ok(Some(source))
}

pub(crate) async fn prepare_with_file(library: &LibrarySession, hash: ContentHash, file: Option<client_platform_web::web_storage::FileReader>) -> Result<(PlaybackSource, PlaybackLease), BackendError> {
    let format = library.db.book_format(&hash).map_err(BackendError::operation)?;
    if !matches!(format, Some(book_model::BookFormat::M4b | book_model::BookFormat::Mp3Folder)) {
        return Err("book is not an audiobook".into());
    }
    let metadata = library.db.audiobook_playback_metadata(&hash).map_err(BackendError::operation)?;
    let remote = if file.is_none() && (format == Some(book_model::BookFormat::Mp3Folder) || metadata.is_none()) { Some(library.describe_remote_file(hash).await.map_err(BackendError::operation)?.0) } else { None };
    let current_hash = if format == Some(book_model::BookFormat::Mp3Folder) {
        match (&file, &remote) {
            (Some(file), _) => file.content_hash().map_err(BackendError::message)?,
            (_, Some(remote)) => Some(remote.hash),
            _ => None,
        }
    } else {
        None
    };
    // Browser playback currently runs without artwork. In particular, opening
    // a remote book must not wait on optional thumbnail extraction or storage,
    // so the cover stays empty.
    let metadata = metadata
        .filter(|metadata| format != Some(book_model::BookFormat::Mp3Folder) || current_hash.is_some_and(|content_hash| book_model::tracks_match_archive(&metadata.tracks, content_hash)))
        .ok_or("audiobook addition has no valid playback facts")?;
    let (location, local_lease) = match file {
        Some(file) if format == Some(book_model::BookFormat::Mp3Folder) => {
            if metadata.tracks.is_empty() {
                return Err("downloaded MP3 folder has no track index".into());
            }
            let (urls, lease) = client_platform_web::transport::resources::local_track_media_lease(file, &metadata.tracks).await.map_err(BackendError::operation)?;
            (PlaybackLocation::LocalTracks(urls), Some(lease))
        }
        Some(file) => {
            let (url, lease) = client_platform_web::transport::resources::local_media_lease(file).await.map_err(BackendError::operation)?;
            (PlaybackLocation::LocalUrl(url), Some(lease))
        }
        None => {
            let source = library.direct_audio_source(hash).await?;
            if format == Some(book_model::BookFormat::Mp3Folder) && !book_model::tracks_match_archive(&metadata.tracks, source.content_hash) {
                return Err("audiobook track index does not match the playback archive".into());
            }
            (PlaybackLocation::Direct(source), None)
        }
    };
    let lease = PlaybackLease::local(local_lease);
    Ok((PlaybackSource { metadata, location }, lease))
}
