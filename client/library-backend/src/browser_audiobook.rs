//! Local media retains a storage lease; remote media uses expiring direct URLs.
use crate::library::PlaybackLocation;
pub use crate::library::{PlaybackMetadata, PlaybackSource};
use crate::worker_cpu::{BookOperation, CpuRequest, Inspect};
use crate::{ContentHash, LibrarySession};
use wasm_bindgen::prelude::*;

use client_platform_web::transport::range::Server as RangeServer;
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

pub(crate) async fn prepare(library: &LibrarySession, hash: ContentHash) -> Result<(PlaybackSource, PlaybackLease), crate::BackendError> {
    let file = library.assets.playback_reader(&hash).await.map_err(crate::BackendError::operation)?;
    prepare_with_file(library, hash, file).await
}

/// Browser-host command used when a local audiobook is absent.  The format
/// check and storage probe remain inside the owning library module.
pub async fn open_remote_audio(library: &LibrarySession, hash: ContentHash) -> Result<Option<PlaybackSource>, crate::BackendError> {
    let format = library.db.book_format(&hash).map_err(crate::BackendError::operation)?;
    if format != Some(book_model::BookFormat::M4b) || library.assets.playback_reader(&hash).await.map_err(crate::BackendError::operation)?.is_some() {
        return Ok(None);
    }
    let (source, _lease) = prepare(library, hash).await?;
    Ok(Some(source))
}

pub(crate) async fn prepare_with_file(library: &LibrarySession, hash: ContentHash, file: Option<client_platform_web::web_storage::FileReader>) -> Result<(PlaybackSource, PlaybackLease), crate::BackendError> {
    if library.db.book_format(&hash).map_err(crate::BackendError::operation)? != Some(book_model::BookFormat::M4b) {
        return Err("book is not an audiobook".into());
    }
    let snapshot = library.db.audiobook_playback_snapshot(&hash).map_err(crate::BackendError::operation)?;
    let (title, author) = (snapshot.title, snapshot.author);
    let cached = snapshot.cached.map(|metadata| (metadata.duration_ms, metadata.chapters, metadata.narrator));
    let remote = if file.is_none() && cached.is_none() { Some(library.describe_remote_file(hash).await?.0) } else { None };
    let (duration_ms, chapters, narrator) = if let Some(cached) = cached {
        cached
    } else {
        let inspection = book(library, hash, remote.as_ref(), Inspect { source_name: format!("{title}.m4b"), format: book_model::BookFormat::M4b }).await?;
        let audio = inspection.audiobook.ok_or("missing audiobook metadata")?;
        let committed = library.db.commit_audiobook_metadata(&hash, &audio).map_err(crate::BackendError::operation)?;
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

async fn book<C: BookOperation>(library: &LibrarySession, hash: ContentHash, remote: Option<&crate::library::RemoteFile>, task: C) -> Result<C::Reply, crate::BackendError> {
    if let Some(remote) = remote {
        // Only parser-requested ranges cross to the CPU worker. The library
        // retains authentication and pins every read to this remote revision.
        let account = library.sync().and_then(|sync| sync.account()).ok_or("library account access is unavailable")?;
        let library_id = *library.id();
        let remote = (*remote).clone();
        let client = reqwest::Client::new();
        let source = RangeServer::asynchronous(remote.length, move |offset, length| {
            let account = account.clone();
            let remote = remote.clone();
            let client = client.clone();
            async move { crate::library::remote_access::read_remote_file(&client, &account, library_id, &remote, offset, length).await }
        })
        .map_err(crate::BackendError::operation)?;
        submit_source(task, source.source()).await
    } else {
        let reader = library.assets.prepare_reader(crate::BlobKind::Book, &hash).await.map_err(crate::BackendError::operation)?.ok_or("audiobook file disappeared")?;
        submit_book(task, reader).await
    }
}

async fn submit_book<C: BookOperation>(operation: C, reader: crate::BoxedBookReader) -> Result<C::Reply, crate::BackendError> {
    let server = client_platform_web::cpu_host::serve_reader(reader).map_err(crate::BackendError::operation)?;
    let bytes = rmp_serde::to_vec_named(&CpuRequest::Book { task: operation.task() }).map_err(crate::BackendError::operation)?;
    let response = client_platform_web::transport::cpu_client::request_with_source(bytes, server.source()).map_err(crate::BackendError::operation)?.await.map_err(crate::BackendError::operation)?;
    C::decode(rmp_serde::from_slice(&response).map_err(crate::BackendError::operation)?)
}

async fn submit_source<C: crate::worker_cpu::Operation>(operation: C, source: JsValue) -> Result<C::Reply, crate::BackendError> {
    let bytes = rmp_serde::to_vec_named(&operation.request()).map_err(crate::BackendError::operation)?;
    let response = client_platform_web::transport::cpu_client::request_with_source(bytes, source).map_err(crate::BackendError::operation)?.await.map_err(crate::BackendError::operation)?;
    C::decode(rmp_serde::from_slice(&response).map_err(crate::BackendError::operation)?)
}
