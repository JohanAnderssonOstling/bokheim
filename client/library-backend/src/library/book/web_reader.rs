//! Browser book-opening policy over library commands.

use crate::library::{PendingCommand, ReadyCommand};

use std::future::Future;

use crate::ContentHash;
use crate::library::PlaybackLocation::Direct;
use crate::library::ResolvedBook;
pub use book_access::remote_file::BrowserReader as Reader;
pub use client_platform_web::transport::range::connect;

use crate::library::commands::{LibraryCommand, LibraryReply};
use crate::{BookSource, ResolvedBookData};

/// Library-scoped command transport supplied by the application host.
pub trait BookTransport: Clone + 'static {
    fn request<T: LibraryReply>(&self, command: LibraryCommand) -> impl Future<Output = Result<T, String>>;
    fn subscribe_book(&self, content_hash: ContentHash) -> impl Future<Output = Result<(async_channel::Receiver<()>, ResolvedBookData), String>>;
}

fn audiobook<T: BookTransport>(source: crate::library::PlaybackSource, lease: Option<async_channel::Receiver<()>>, library: T, content_hash: ContentHash) -> ResolvedBook {
    let renew = matches!(&source.location, Direct(_)).then(|| {
        client_platform_web::transport::resources::AsyncCallback::new(move || {
            let library = library.clone();
            async move {
                let direct: crate::library::DirectPlayback =
                    library.request(LibraryCommand::Pending(PendingCommand::RenewRemoteAudio { content_hash })).await.map_err(|error| wasm_bindgen::JsValue::from_str(&error))?;
                js_sys::JSON::parse(&serde_json::to_string(&direct).unwrap())
            }
        })
    });
    let (url, direct, local_tracks) = match source.location {
        crate::library::PlaybackLocation::LocalUrl(url) => (url, None, Vec::new()),
        crate::library::PlaybackLocation::LocalTracks(tracks) => (String::new(), None, tracks),
        Direct(direct) => (direct.url.clone(), Some(direct), Vec::new()),
    };
    ResolvedBook::Audiobook(crate::library::browser_audiobook::BrowserAudiobook::new(source.metadata, url, local_tracks, direct, lease, renew))
}

pub async fn resolve_book<T: BookTransport>(library: &T, content_hash: ContentHash) -> Result<ResolvedBook, String> {
    if let Some(source) = library.request(LibraryCommand::Pending(PendingCommand::OpenRemoteAudio { content_hash })).await? {
        return Ok(audiobook(source, None, library.clone(), content_hash));
    }
    let (lease, resolved) = crate::executor::timeout(std::time::Duration::from_secs(65), library.subscribe_book(content_hash)).await.map_err(|_| "opening the book source timed out".to_owned())??;
    if matches!(resolved.format, book_model::BookFormat::Epub | book_model::BookFormat::Mobi) {
        let source = match resolved.source {
            BookSource::Local { capability, length } => {
                let library = library.clone();
                book_access::AsyncBookSource::browser(length, Vec::new(), move |offset, length| {
                    let library = library.clone();
                    let capability = capability.clone();
                    let lease = lease.clone();
                    Box::pin(async move {
                        let _lease = lease;
                        let bytes: serde_bytes::ByteBuf = library.request(LibraryCommand::Ready(ReadyCommand::ReadBookRange { capability, offset, length })).await?;
                        Ok(bytes.into_vec())
                    })
                })
                .map_err(|error| error.to_string())?
            }
            BookSource::Remote { source, prefix } => {
                let library = library.clone();
                book_access::AsyncBookSource::browser(source.length, prefix, move |offset, length| {
                    let library = library.clone();
                    let source = source.clone();
                    let lease = lease.clone();
                    Box::pin(async move {
                        let _lease = lease;
                        let bytes: serde_bytes::ByteBuf = library.request(LibraryCommand::Pending(PendingCommand::ReadFileRange { source, offset, length })).await?;
                        Ok(bytes.into_vec())
                    })
                })
                .map_err(|error| error.to_string())?
            }
            _ => return Err("browser received an invalid EPUB source".into()),
        };
        return Ok(ResolvedBook::Document(crate::library::DocumentBook::from_async_source(String::new(), resolved.format, source)));
    }
    let mut pdf_page_source = None;
    let reader: crate::BoxedBookReader = match resolved.source {
        BookSource::Audiobook(source) => return Ok(audiobook(source, Some(lease), library.clone(), content_hash)),
        BookSource::Local { capability, length } => {
            let library = library.clone();
            book_access::remote_file::browser_reader(length, Vec::new(), move |offset, length| {
                let library = library.clone();
                let capability = capability.clone();
                let lease = lease.clone();
                Box::pin(async move {
                    let _lease = lease;
                    let bytes: serde_bytes::ByteBuf = library.request(LibraryCommand::Ready(ReadyCommand::ReadBookRange { capability, offset, length })).await?;
                    Ok(bytes.into_vec())
                })
            })
            .map_err(|error| error.to_string())?
        }
        BookSource::Remote { source, prefix } => {
            let library = library.clone();
            if let Some(index) = resolved.pdf_metadata.as_ref().and_then(|m| m.dependencies.clone()).filter(|index| index.length == source.length) {
                let fetch = book_access::remote_file::browser_bundle_fetch(move |ranges| {
                    let library = library.clone();
                    let source = source.clone();
                    Box::pin(async move {
                        let bytes: serde_bytes::ByteBuf = library.request(LibraryCommand::Pending(PendingCommand::ReadFileBundle { source, ranges })).await?;
                        Ok(bytes.into_vec())
                    })
                })
                .map_err(|error| error.to_string())?;
                let (reader, hook) = book_access::pdf_bundle::open(index, prefix, fetch).map_err(|error| error.to_string())?;
                pdf_page_source = Some(hook);
                reader
            } else {
                book_access::remote_file::browser_reader(source.length, prefix, move |offset, length| {
                    let library = library.clone();
                    let source = source.clone();
                    Box::pin(async move {
                        let bytes: serde_bytes::ByteBuf = library.request(LibraryCommand::Pending(PendingCommand::ReadFileRange { source, offset, length })).await?;
                        Ok(bytes.into_vec())
                    })
                })
                .map_err(|error| error.to_string())?
            }
        }
        BookSource::Inline(_) => return Err("browser received bytes instead of a book source".into()),
    };
    let mut document = crate::library::DocumentBook::new(String::new(), resolved.format, reader);
    document.pdf_metadata = resolved.pdf_metadata;
    document.pdf_page_source = pdf_page_source;
    Ok(ResolvedBook::Document(document))
}
