//! Browser book-opening policy over library commands.

use crate::library::{PendingCommand, ReadyCommand};

use std::future::Future;

use crate::BookId;
use crate::library::PlaybackLocation::Direct;
use crate::library::ResolvedBook;
pub use book_access::remote_file::BrowserReader as Reader;
use book_access::{FetchFuture, SparseBookSource};
use book_model::BookFormat;
pub use client_platform_web::transport::range::connect;

use crate::library::commands::{LibraryCommand, LibraryReply};
use crate::{BackendError, BookSource, ResolvedBookData};
use client_runtime::Failure;

/// Library-scoped command transport supplied by the application host.
pub trait BookTransport: Clone + 'static {
    fn request<T: LibraryReply>(&self, command: LibraryCommand) -> impl Future<Output = Result<T, BackendError>>;
    fn subscribe_book(&self, content_hash: BookId) -> impl Future<Output = Result<(async_channel::Receiver<()>, ResolvedBookData), BackendError>>;
}

/// Keep the subscription alive for every in-flight read as well as the fetcher.
fn range_fetch<T: BookTransport>(library: T, lease: Option<async_channel::Receiver<()>>, command: impl Fn(u64, usize) -> LibraryCommand + 'static) -> impl Fn(u64, usize) -> FetchFuture<'static, Result<Vec<u8>, String>> {
    move |offset, length| {
        let library = library.clone();
        let lease = lease.clone();
        let command = command(offset, length);
        Box::pin(async move {
            let _lease = lease;
            let bytes: serde_bytes::ByteBuf = library.request(command).await.map_err(|error| error.to_string())?;
            Ok(bytes.into_vec())
        })
    }
}

async fn audiobook<T: BookTransport>(source: crate::library::PlaybackSource, lease: Option<async_channel::Receiver<()>>, library: T, content_hash: BookId) -> Result<ResolvedBook, BackendError> {
    let renewal_library = library.clone();
    let renew = matches!(&source.location, Direct(_)).then(|| {
        client_platform_web::transport::resources::AsyncCallback::new(move || {
            let library = renewal_library.clone();
            async move {
                let direct: crate::library::DirectPlayback = library.request(LibraryCommand::Pending(PendingCommand::RenewRemoteAudio { content_hash })).await.map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
                let json = serde_json::to_string(&direct).map_err(|error| wasm_bindgen::JsValue::from_str(&format!("Invalid direct playback configuration: {error}")))?;
                js_sys::JSON::parse(&json)
            }
        })
    });
    let (url, direct, local_tracks) = match source.location {
        crate::library::PlaybackLocation::LocalUrl(url) => (url, None, Vec::new()),
        crate::library::PlaybackLocation::LocalTracks(tracks) => (String::new(), None, tracks),
        Direct(direct) => (direct.url.clone(), Some(direct), Vec::new()),
    };
    Ok(ResolvedBook::Audiobook(crate::library::browser_audiobook::BrowserAudiobook::new(source.metadata, url, local_tracks, direct, lease, renew)))
}

pub async fn resolve_book<T: BookTransport>(library: &T, content_hash: BookId) -> Result<ResolvedBook, BackendError> {
    if let Some(source) = library.request(LibraryCommand::Pending(PendingCommand::OpenRemoteAudio { content_hash })).await? {
        return audiobook(source, None, library.clone(), content_hash).await;
    }
    let (lease, resolved) = crate::executor::timeout(std::time::Duration::from_secs(65), library.subscribe_book(content_hash)).await.map_err(|_| BackendError::classified(Failure::TimedOut, "opening the book source timed out"))??;
    if matches!(resolved.format, BookFormat::Epub | BookFormat::Mobi | BookFormat::Fb2 | BookFormat::Cbz) {
        let source = match resolved.source {
            BookSource::Local { capability, length } => {
                let fetch = range_fetch(library.clone(), Some(lease), move |offset, length| LibraryCommand::Ready(ReadyCommand::ReadBookRange { capability: capability.clone(), offset, length }));
                book_access::AsyncBookSource::browser(length, Vec::new(), fetch)?
            }
            BookSource::RemoteArchive { source, tail } if matches!(resolved.format, BookFormat::Epub | BookFormat::Cbz) => {
                let range_source = source.clone();
                let fetch = range_fetch(library.clone(), None, move |offset, length| LibraryCommand::Pending(PendingCommand::ReadFileRange { source: range_source.clone(), offset, length }));
                let seed_offset = source.length.checked_sub(tail.len() as u64).ok_or("invalid ZIP tail")?;
                let sparse = SparseBookSource::browser(source.length, seed_offset, tail, fetch)?;
                let batch_library = library.clone();
                let batch_source = source.clone();
                let sparse = sparse.with_browser_prefetch(move |ranges| {
                    let library = batch_library.clone();
                    let source = batch_source.clone();
                    Box::pin(async move {
                        let bytes: serde_bytes::ByteBuf = library.request(LibraryCommand::Pending(PendingCommand::ReadFileRanges { source, ranges })).await.map_err(|error| error.to_string())?;
                        Ok(bytes.into_vec())
                    })
                });
                if resolved.format == BookFormat::Cbz { book_access::AsyncBookSource::from(sparse) } else { super::remote_archive::prepare_epub(sparse).await? }
            }
            BookSource::Remote { source, prefix } => {
                let length = source.length;
                let fetch = range_fetch(library.clone(), Some(lease), move |offset, length| LibraryCommand::Pending(PendingCommand::ReadFileRange { source: source.clone(), offset, length }));
                book_access::AsyncBookSource::browser(length, prefix, fetch)?
            }
            _ => return Err("browser received an invalid book source".into()),
        };
        return Ok(ResolvedBook::Document(crate::library::DocumentBook::from_async_source(String::new(), resolved.format, source)));
    }
    let reader: crate::BoxedBookReader = match resolved.source {
        BookSource::Audiobook(source) => return audiobook(source, Some(lease), library.clone(), content_hash).await,
        BookSource::Local { capability, length } => {
            let fetch = range_fetch(library.clone(), Some(lease), move |offset, length| LibraryCommand::Ready(ReadyCommand::ReadBookRange { capability: capability.clone(), offset, length }));
            book_access::remote_file::browser_reader(length, Vec::new(), fetch)?
        }
        BookSource::Remote { source, prefix } => {
            let library = library.clone();
            if resolved.format == BookFormat::Pdf {
                let multipart = source.revision.is_none();
                let length = source.length;
                let fetch = book_access::remote_file::browser_bundle_fetch(move |ranges| {
                    let library = library.clone();
                    let source = source.clone();
                    Box::pin(async move {
                        let command = if multipart { PendingCommand::ReadFileRanges { source, ranges } } else { PendingCommand::ReadFileBundle { source, ranges } };
                        let bytes: serde_bytes::ByteBuf = library.request(LibraryCommand::Pending(command)).await.map_err(|error| error.to_string())?;
                        Ok(bytes.into_vec())
                    })
                })?;
                book_access::pdf_bundle::open(length, prefix, fetch).map_err(|error| error.to_string())?
            } else {
                let length = source.length;
                let fetch = range_fetch(library, None, move |offset, length| LibraryCommand::Pending(PendingCommand::ReadFileRange { source: source.clone(), offset, length }));
                book_access::remote_file::browser_reader(length, prefix, fetch)?
            }
        }
        BookSource::RemoteArchive { .. } => return Err("browser received a ZIP source for another format".into()),
        BookSource::Inline(_) => return Err("browser received bytes instead of a book source".into()),
    };
    let mut document = crate::library::DocumentBook::new(String::new(), resolved.format, reader);
    document.pdf_metadata = resolved.pdf_metadata;
    Ok(ResolvedBook::Document(document))
}
