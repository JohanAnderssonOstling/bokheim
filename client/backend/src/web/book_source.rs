//! Book reader and cache policy over browser range transport.
pub use client_platform_web::transport::range::connect;
use client_platform_web::transport::range::{Server as RangeServer, Source};
use std::io::{Read, Seek, SeekFrom};
use wasm_bindgen::JsValue;

pub(crate) struct Server(RangeServer);
impl Server {
    pub fn new(mut reader: crate::BoxedBookReader) -> Result<Self, String> {
        let length = reader.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        RangeServer::sync(length, move |offset, length| {
            let mut bytes = vec![0; length.min(256 * 1024)];
            reader.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;
            let count = reader.read(&mut bytes).map_err(|e| e.to_string())?;
            bytes.truncate(count);
            Ok(bytes)
        })
        .map(Self)
    }
    pub fn source(&self) -> JsValue {
        self.0.source()
    }
}

pub struct Reader {
    source: Source,
    cache: crate::range_cache::RangeCache,
    position: u64,
    length: u64,
}
impl Reader {
    #[cfg(feature = "web-runtime-tests")]
    pub fn cache_bytes(&self) -> usize {
        self.cache.allocated_bytes()
    }
    pub fn new(source: JsValue) -> Result<Self, String> {
        let source = Source::new(source)?;
        let length = source.length();
        Ok(Self { source, cache: crate::range_cache::RangeCache::new(4), position: 0, length })
    }
}
impl Read for Reader {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        let count = bytes.len().min(256 * 1024).min(self.length.saturating_sub(self.position).min(256 * 1024) as usize);
        if count == 0 {
            return Ok(0);
        }
        // A cache hit must still reject a closed source.
        self.source.check()?;
        let mut copied = 0;
        while copied < count {
            let position = self.position + copied as u64;
            let cached = self.cache.read(position, &mut bytes[copied..count]);
            if cached > 0 {
                copied += cached;
                continue;
            }
            let mut start = position / (64 * 1024) * (64 * 1024);
            let fetch = |start: u64| -> std::io::Result<Vec<u8>> {
                let length = self.length.saturating_sub(start).min(64 * 1024) as u32;
                let chunk = self.source.read(start, length)?;
                if chunk.len() > length as usize {
                    return Err(std::io::Error::other("invalid book read length"));
                }
                Ok(chunk)
            };
            let mut chunk = fetch(start)?;
            if start + chunk.len() as u64 <= position && start != position {
                start = position;
                chunk = fetch(start)?;
            }
            if chunk.is_empty() {
                break;
            }
            self.cache.insert(start, chunk);
        }
        self.position += copied as u64;
        Ok(copied)
    }
}
impl Seek for Reader {
    fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
        let position = match from {
            SeekFrom::Start(n) => n as i128,
            SeekFrom::Current(n) => self.position as i128 + n as i128,
            SeekFrom::End(n) => self.length as i128 + n as i128,
        };
        self.position = u64::try_from(position).map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid book seek"))?;
        Ok(self.position)
    }
}
/// Authentication and fetch policy are supplied by the backend.
pub(crate) struct AsyncServer(RangeServer);
impl AsyncServer {
    pub fn new<F>(length: u64, fetch: F) -> Result<Self, String>
    where
        F: FnMut(u64, usize) -> crate::executor::BoxedBackendFuture<'static, Result<Vec<u8>, String>> + 'static,
    {
        RangeServer::asynchronous(length, fetch).map(Self)
    }
    pub fn source(&self) -> JsValue {
        self.0.source()
    }
}

use crate::runtime::{BookSource, ResolvedBookData, library_requests};

fn audiobook(source: crate::library::PlaybackSource, lease: Option<async_channel::Receiver<()>>, library: crate::LibraryClient, content_hash: crate::ContentHash) -> crate::library::ResolvedBook {
    let renew = matches!(&source.location, crate::library::PlaybackLocation::Direct(_)).then(|| {
        client_platform_web::transport::resources::AsyncCallback::new(move || {
            let library = library.clone();
            async move {
                let direct: crate::library::DirectPlayback = library.request(library_requests::RenewRemoteAudio { content_hash }).await.map_err(|error| wasm_bindgen::JsValue::from_str(&error))?;
                js_sys::JSON::parse(&serde_json::to_string(&direct).unwrap())
            }
        })
    });
    crate::library::ResolvedBook::Audiobook(crate::library::browser_audiobook::BrowserAudiobook::new(source, lease, renew))
}

pub(crate) async fn resolve_book(library: &crate::LibraryClient, content_hash: crate::ContentHash) -> Result<crate::library::ResolvedBook, String> {
    if let Some(source) = library.request(super::requests::library_requests::OpenRemoteAudio { content_hash }).await? {
        return Ok(audiobook(source, None, library.clone(), content_hash));
    }
    let (lease, resolved) = crate::executor::timeout(std::time::Duration::from_secs(65), library.app.transport.web_typed_subscription::<(), ResolvedBookData>(super::requests::WebAppCommand::SubscribeBook { library_id: library.library_id, content_hash }.into()))
        .await
        .map_err(|_| "opening the book source timed out".to_owned())??;
    let mut pdf_page_source = None;
    let reader: crate::BoxedBookReader = match resolved.source {
        BookSource::Audiobook(source) => {
            return Ok(audiobook(source, Some(lease), library.clone(), content_hash));
        }
        BookSource::Local { capability, length } => {
            let library = library.clone();
            book_access::remote_file::browser_reader(length, None, move |offset, length| {
                let library = library.clone();
                let capability = capability.clone();
                let lease = lease.clone();
                Box::pin(async move {
                    let _lease = lease;
                    let bytes: serde_bytes::ByteBuf = library.request(super::requests::library_requests::ReadBookRange { capability, offset, length }).await?;
                    Ok(bytes.into_vec())
                })
            })?
        }
        BookSource::Remote { source, prefix } => {
            let library = library.clone();
            if let Some(index) = resolved.pdf_metadata.as_ref().and_then(|m| m.dependencies.clone()).filter(|index| index.length == source.length) {
                let fetch = book_access::remote_file::browser_bundle_fetch(move |ranges| {
                    let library = library.clone();
                    let source = source.clone();
                    Box::pin(async move {
                        let bytes: serde_bytes::ByteBuf = library.request(library_requests::ReadFileBundle { source, ranges }).await?;
                        Ok(bytes.into_vec())
                    })
                })?;
                let (reader, hook) = book_access::pdf_bundle::open(index, prefix, fetch)?;
                pdf_page_source = Some(hook);
                reader
            } else {
                book_access::remote_file::browser_reader(source.length, Some(prefix), move |offset, length| {
                    let library = library.clone();
                    let source = source.clone();
                    Box::pin(async move {
                        let bytes: serde_bytes::ByteBuf = library.request(library_requests::ReadFileRange { source, offset, length }).await?;
                        Ok(bytes.into_vec())
                    })
                })?
            }
        }
        BookSource::Inline(_) => return Err("browser received bytes instead of a book source".into()),
    };
    let mut document = crate::library::DocumentBook::new(String::new(), resolved.format, reader);
    document.pdf_metadata = resolved.pdf_metadata;
    document.pdf_page_source = pdf_page_source;
    Ok(crate::library::ResolvedBook::Document(document))
}

#[cfg(feature = "web-runtime-tests")]
impl crate::LibraryClient {
    /// A page cannot perform blocking parser reads. Browser probes exercise the
    /// same subscription and range commands asynchronously instead.
    pub async fn book_prefix(&self, content_hash: crate::ContentHash, length: usize) -> Result<Vec<u8>, String> {
        let (_lease, book) = self.app.transport.web_typed_subscription::<(), ResolvedBookData>(super::requests::WebAppCommand::SubscribeBook { library_id: self.library_id, content_hash }.into()).await?;
        let BookSource::Local { capability, .. } = book.source else {
            return Err("expected a local document".into());
        };
        let bytes: serde_bytes::ByteBuf = self.request(super::requests::library_requests::ReadBookRange { capability, offset: 0, length }).await?;
        Ok(bytes.into_vec())
    }
}

impl crate::LibraryClient {
    pub fn resolve_book_diagnostics(&self) -> async_channel::Receiver<String> {
        self.app.transport.web_diagnostics()
    }
}
