//! Local media retains a storage lease; remote media uses expiring direct URLs.
use crate::library::PlaybackLocation;
pub use crate::library::{PlaybackMetadata, PlaybackSource};
use crate::worker_cpu::{Inspect, BookOperation};
use crate::{ContentHash, LibrarySession};
use wasm_bindgen::prelude::*;

use client_platform_web::transport::resources::{AsyncCallback, BlobUrl};

/// Dropped by the worker when its owning tab cancels the subscription or disconnects.
pub struct PlaybackLease {
    _url: Option<BlobUrl>,
    _file: Option<client_platform_web::web_storage::FileReader>,
}
pub struct BrowserAudiobook {
    pub metadata: PlaybackMetadata,
    pub url: String,
    pub direct: Option<crate::library::DirectPlayback>,
    renew: Option<AsyncCallback>,
    _lease: Option<async_channel::Receiver<()>>,
}
impl BrowserAudiobook {
    pub fn renewal(&self) -> JsValue {
        self.renew.as_ref().map(|renew| renew.value()).unwrap_or(JsValue::UNDEFINED)
    }
    pub fn direct_config(&self) -> Result<JsValue, String> {
        self.direct.as_ref().map(|config| js_sys::JSON::parse(&serde_json::to_string(config).unwrap()).map_err(|_| "Invalid playback configuration".into())).unwrap_or(Ok(JsValue::UNDEFINED))
    }
    pub fn new(source: PlaybackSource, lease: Option<async_channel::Receiver<()>>, renew: Option<AsyncCallback>) -> Self {
        let mut result = Self { metadata: source.metadata, url: String::new(), direct: None, renew: None, _lease: lease };
        match source.location {
            PlaybackLocation::LocalUrl(url) => {
                result.url = url;
            }
            PlaybackLocation::Direct(direct) => {
                result._lease = None;
                result.url = direct.url.clone();
                result.direct = Some(direct);
                result.renew = renew;
            }
        }
        result
    }
}

pub(crate) async fn prepare(library: &LibrarySession, hash: ContentHash) -> Result<(PlaybackSource, PlaybackLease), String> {
    let file = library.assets.playback_reader(&hash).await.map_err(|e| e.to_string())?;
    prepare_with_file(library, hash, file).await
}

/// Browser-host command used when a local audiobook is absent.  The format
/// check and storage probe remain inside the owning library module.
pub async fn open_remote_audio(library: &LibrarySession, hash: ContentHash) -> Result<Option<PlaybackSource>, String> {
    let format = library.db.book_format(&hash).map_err(|error| error.to_string())?;
    if format != Some(book_model::BookFormat::M4b) || library.assets.playback_reader(&hash).await.map_err(|error| error.to_string())?.is_some() {
        return Ok(None);
    }
    let (source, _lease) = prepare(library, hash).await?;
    Ok(Some(source))
}

pub(crate) async fn prepare_with_file(library: &LibrarySession, hash: ContentHash, file: Option<client_platform_web::web_storage::FileReader>) -> Result<(PlaybackSource, PlaybackLease), String> {
    if library.db.book_format(&hash).map_err(|error| error.to_string())? != Some(book_model::BookFormat::M4b) {
        return Err("book is not an audiobook".into());
    }
    let snapshot = library.db.audiobook_playback_snapshot(&hash).map_err(|error| error.to_string())?;
    let (title, author) = (snapshot.title, snapshot.author);
    let cached = snapshot.cached.map(|metadata| (metadata.duration_ms, metadata.chapters, metadata.narrator));
    let remote = if file.is_none() && cached.is_none() { Some(library.describe_remote_file(hash).await?.0) } else { None };
    let (duration_ms, chapters, narrator) = if let Some(cached) = cached {
        cached
    } else {
        let inspection = book(library, hash, remote.as_ref(), Inspect { source_name: format!("{title}.m4b"), format: book_model::BookFormat::M4b }).await?;
        let audio = inspection.audiobook.ok_or("missing audiobook metadata")?;
        let committed = library.db.commit_audiobook_metadata(&hash, &audio).map_err(|error| error.to_string())?;
        (committed.duration_ms, committed.chapters, committed.narrator)
    };
    // Browser playback currently runs without artwork. In particular, opening
    // a remote book must not wait on optional thumbnail extraction or storage.
    let cover = Vec::new();
    let url = if let Some(file) = &file { Some(BlobUrl::new(&file.upload_file().await?).map_err(|e| format!("Could not open audiobook file: {e:?}"))?) } else { None };
    let location = match &url {
        Some(url) => PlaybackLocation::LocalUrl(url.url().to_owned()),
        None => PlaybackLocation::Direct(library.direct_audio_source(hash).await?),
    };
    let lease = PlaybackLease { _url: url, _file: file };
    Ok((PlaybackSource { metadata: PlaybackMetadata { title, author, narrator, duration_ms, chapters, cover }, location }, lease))
}

async fn book<C: BookOperation>(library: &LibrarySession, hash: ContentHash, remote: Option<&crate::library::RemoteFile>, task: C) -> Result<C::Reply, String> {
    if let Some(remote) = remote {
        let _ = remote;
        Err("remote audiobook inspection waits for library download".into())
    } else {
        let reader = library.assets.prepare_reader(crate::BlobKind::Book, &hash).await.map_err(|e| e.to_string())?.ok_or("audiobook file disappeared")?;
        crate::worker_cpu::submit_book(task, reader).await
    }
}
