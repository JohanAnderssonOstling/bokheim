//! Local media retains a storage lease; remote media uses expiring direct URLs.
use crate::library::PlaybackLocation;
pub use crate::library::{PlaybackMetadata, PlaybackSource};
use crate::runtime::library_requests;
use crate::worker_cpu::{Inspect, PublicationOperation};
use crate::{ContentHash, LibrarySession};
use wasm_bindgen::prelude::*;

use client_platform_web::transport::resources::{AsyncCallback, BlobUrl};

/// Dropped by the worker when its owning tab cancels the subscription or disconnects.
pub struct PlaybackLease {
    _url: Option<BlobUrl>,
    _file: Option<crate::platform::web_storage::FileReader>,
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
    pub(crate) fn new(source: PlaybackSource, lease: Option<async_channel::Receiver<()>>, library: crate::LibraryClient, hash: ContentHash) -> Self {
        let mut result = Self { metadata: source.metadata, url: String::new(), direct: None, renew: None, _lease: lease };
        match source.location {
            PlaybackLocation::LocalUrl(url) => {
                result.url = url;
            }
            PlaybackLocation::Direct(direct) => {
                result._lease = None;
                result.url = direct.url.clone();
                result.direct = Some(direct);
                result.renew = Some(AsyncCallback::new(move || {
                    let library = library.clone();
                    async move {
                        let direct: crate::library::DirectPlayback = library.request(library_requests::RenewRemoteAudio { content_hash: hash }).await.map_err(|error| JsValue::from_str(&error))?;
                        js_sys::JSON::parse(&serde_json::to_string(&direct).unwrap())
                    }
                }));
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
pub(crate) async fn open_remote_audio(library: &LibrarySession, hash: ContentHash) -> Result<Option<PlaybackSource>, String> {
    let format = client_library::assets::get_book_format(&library.db.get().map_err(|error| error.to_string())?, &hash).map_err(|error| error.to_string())?;
    if format != Some(library_model::BookFormat::M4b) || library.assets.playback_reader(&hash).await.map_err(|error| error.to_string())?.is_some() {
        return Ok(None);
    }
    let (source, _lease) = prepare(library, hash).await?;
    Ok(Some(source))
}

pub(crate) async fn prepare_with_file(library: &LibrarySession, hash: ContentHash, file: Option<crate::platform::web_storage::FileReader>) -> Result<(PlaybackSource, PlaybackLease), String> {
    let (title, author, cached) = {
        let conn = library.db.get().map_err(|e| e.to_string())?;
        if client_library::assets::get_book_format(&conn, &hash).map_err(|e| e.to_string())? != Some(library_model::BookFormat::M4b) {
            return Err("book is not an audiobook".into());
        }
        match client_library::audiobooks::cached_playback_metadata(&conn, hash).map_err(|e| e.to_string())? {
            Some(cached) => (cached.title, cached.author, Some((cached.duration_ms, cached.chapters, cached.narrator))),
            None => {
                let (title, author) = client_library::audiobooks::playback_title_author(&conn, hash).map_err(|e| e.to_string())?;
                (title, author, None)
            }
        }
    };
    let remote = if file.is_none() && cached.is_none() { Some(library.describe_remote_file(hash).await?.0) } else { None };
    let (duration_ms, chapters, narrator) = if let Some(cached) = cached {
        cached
    } else {
        let inspection = publication(library, hash, remote.as_ref(), Inspect { source_name: format!("{title}.m4b"), format: library_model::BookFormat::M4b }).await?;
        let audio = inspection.audiobook.ok_or("missing audiobook metadata")?;
        {
            let mut conn = library.db.get().map_err(|e| e.to_string())?;
            let tx = client_library::schema::library_write_transaction(&mut conn).map_err(|e| e.to_string())?;
            client_library::audiobooks::upsert_audiobook_metadata(&tx, &hash, &audio).map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
        }
        // Inspection persisted new recording evidence; read the narrator after that commit.
        let narrator = {
            let conn = library.db.get().map_err(|e| e.to_string())?;
            client_library::audiobooks::recording_narrator(&conn, hash).map_err(|e| e.to_string())?
        };
        (audio.duration_ms, audio.chapters.into_iter().map(|c| library_model::AudiobookChapter { title: c.title, start_ms: c.start_ms, end_ms: c.end_ms }).collect(), narrator)
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

async fn publication<C: PublicationOperation>(library: &LibrarySession, hash: ContentHash, remote: Option<&crate::library::RemoteFile>, task: C) -> Result<C::Reply, String> {
    if let Some(remote) = remote {
        let _ = remote;
        Err("remote audiobook inspection waits for library download".into())
    } else {
        let reader = library.assets.prepare_reader(crate::BlobKind::Book, &hash).await.map_err(|e| e.to_string())?.ok_or("audiobook file disappeared")?;
        crate::worker_cpu::submit_publication(task, reader).await
    }
}
