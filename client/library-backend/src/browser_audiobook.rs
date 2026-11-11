//! Local media retains a storage lease. Remote media uses a playback grant.

use crate::asset_store::AssetStore;
use crate::library::DirectPlayback;
pub use crate::library::{PlaybackLocation, PlaybackSource};
use crate::{BackendError, BookId};
use book_access::RemoteFileError;
use book_model::{BookFormat, tracks_match_archive};
use client_platform_web::web_storage::FileReader;
use library_database::DatabaseClient;
use library_model::PlaybackMetadata;
use std::future::Future;

pub type PlaybackLease = client_platform_web::audiobook::PlaybackLease;
pub type BrowserAudiobook = client_platform_web::audiobook::BrowserAudiobook<PlaybackMetadata, DirectPlayback>;

/// The browser fast path authorizes remote playback only after one local probe.
pub(crate) async fn open_remote_audio(db: &DatabaseClient, assets: &AssetStore, book: BookId, remote: impl Future<Output = Result<DirectPlayback, RemoteFileError>>) -> Result<Option<PlaybackSource>, BackendError> {
    let format = db.book_format(&book).await?;
    if !matches!(format, Some(BookFormat::M4b | BookFormat::Mp3Folder)) || assets.playback_reader(&book).await.map_err(BackendError::operation)?.is_some() {
        return Ok(None);
    }
    let metadata = db.audiobook_playback_metadata(&book).await?;
    let (source, _lease) = prepare(metadata, None, remote).await?;
    Ok(Some(source))
}

/// Remote authorization is polled only when no local file was supplied.
pub(crate) async fn prepare(metadata: Option<PlaybackMetadata>, file: Option<FileReader>, remote: impl Future<Output = Result<DirectPlayback, RemoteFileError>>) -> Result<(PlaybackSource, PlaybackLease), BackendError> {
    let mut metadata = metadata.ok_or("audiobook addition has no valid playback facts")?;
    let format = metadata.format;
    if !matches!(format, BookFormat::M4b | BookFormat::Mp3Folder) {
        return Err("book is not an audiobook".into());
    }
    // Playback uses persisted facts without waiting for optional artwork.
    let (location, local_lease) = match file {
        Some(file) if format == BookFormat::Mp3Folder => {
            let current_hash = file.book_id().await.map_err(BackendError::operation)?;
            if !current_hash.is_some_and(|book_id| tracks_match_archive(&metadata.tracks, book_id)) {
                return Err("audiobook addition has no valid playback facts".into());
            }
            let (file, members) = client_platform_runtime::executor::run_blocking(move || {
                let mut file = file;
                let members = audiobook_archive::archived_tracks(&mut file)?;
                Ok::<_, std::io::Error>((file, members))
            })
            .await
            .map_err(BackendError::operation)?
            .map_err(BackendError::operation)?;
            if members.len() != metadata.tracks.len() {
                return Err("local audiobook track count differs from its metadata".into());
            }
            for (track, member) in metadata.tracks.iter_mut().zip(members) {
                if track.name != member.name || track.length != member.length {
                    return Err("local audiobook tracks differ from their metadata".into());
                }
                track.offset = member.offset;
            }
            let (urls, lease) = client_platform_web::transport::resources::local_track_media_lease(file, &metadata.tracks).await.map_err(BackendError::operation)?;
            (PlaybackLocation::LocalTracks(urls), Some(lease))
        }
        Some(file) => {
            let (url, lease) = client_platform_web::transport::resources::local_media_lease(file).await.map_err(BackendError::operation)?;
            (PlaybackLocation::LocalUrl(url), Some(lease))
        }
        None => {
            let source = remote.await.map_err(BackendError::operation)?;
            if format == BookFormat::Mp3Folder && !tracks_match_archive(&metadata.tracks, source.book_id) {
                return Err("audiobook track index does not match the playback archive".into());
            }
            (PlaybackLocation::Direct(source), None)
        }
    };
    let lease = PlaybackLease::local(local_lease);
    Ok((PlaybackSource { metadata, location }, lease))
}
