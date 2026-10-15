//! The publications this session has listed, by their card key: what adding
//! one or drawing its cover needs, which a card does not carry. Like the
//! page table, it lives as long as the process.
use crate::fetch::{self, Access, Failure};
use crate::{PDF, Publication};
use library_model::{CardKey, OpdsCredentials, OpdsSource};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use url::Url;

struct Listed {
    file_stem: String,
    download: Option<(Url, &'static str)>,
    buy: Option<Url>,
    cover: Option<Url>,
    description: String,
}

static LISTED: LazyLock<Mutex<HashMap<CardKey, Listed>>> = LazyLock::new(Default::default);
/// Covers as JPEG thumbnails, by card and width; `None` for one that could
/// not be had. A failed fetch is not remembered.
static COVERS: LazyLock<Mutex<HashMap<(CardKey, u32, bool), Option<Vec<u8>>>>> = LazyLock::new(Default::default);
static COVER_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Drop cover bytes while keeping feed descriptions and acquisition links.
pub fn evict_cover_cache() {
    COVER_GENERATION.fetch_add(1, Ordering::SeqCst);
    COVERS.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
}

pub(crate) fn remember(key: CardKey, publication: &Publication) {
    let name = publication.title.replace(['/', '\\', ':'], " ");
    let file_stem = if publication.authors.is_empty() { name } else { format!("{} - {name}", publication.authors.join(", ")) };
    let listed = Listed {
        file_stem: file_stem.trim().to_owned(),
        download: publication.download.as_ref().map(|download| (download.href.clone(), download.media_type)),
        buy: publication.buy.clone(),
        cover: publication.cover.clone(),
        description: publication.description.clone(),
    };
    LISTED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).insert(key, listed);
}

fn listed<T>(key: CardKey, read: impl FnOnce(&Listed) -> T) -> Option<T> {
    LISTED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).get(&key).map(read)
}

/// The feed's description of a listed publication, as HTML: what its page
/// shows. Read from the listing, so it costs no request.
pub fn description(key: CardKey) -> Result<String, String> {
    listed(key, |listed| listed.description.clone()).ok_or_else(|| "this book is no longer listed; open its page again".to_owned())
}

/// The store page that sells a listed publication.
pub fn buy_page(key: CardKey) -> Result<Url, String> {
    listed(key, |listed| listed.buy.clone()).ok_or("this book is no longer listed; open its page again")?.ok_or_else(|| "this book is not offered for sale".to_owned())
}

/// The publication's open download, with the file name to import it under.
pub async fn download(source: &OpdsSource, credentials: Option<&OpdsCredentials>, key: CardKey) -> Result<(String, Vec<u8>), String> {
    let (file_stem, (href, media_type)) = listed(key, |listed| Some((listed.file_stem.clone(), listed.download.clone()?))).ok_or("this book is no longer listed; open its page again")?.ok_or("this book has no open download")?;
    let root = Url::parse(&source.url).map_err(|error| error.to_string())?;
    let bytes = match fetch::bytes(&href, &Access::new(&root, credentials)).await {
        Ok(bytes) => bytes,
        Err(Failure::SignIn) => return Err("the feed asks to sign in".into()),
        Err(Failure::Other(error)) => return Err(error),
    };
    let extension = if media_type == PDF { "pdf" } else { "epub" };
    Ok((format!("{file_stem}.{extension}"), bytes))
}

/// The publication's cover as a JPEG `width` pixels wide, or none when it
/// has none or it cannot be read.
pub async fn cover(source: &OpdsSource, credentials: Option<&OpdsCredentials>, key: CardKey, width: u32) -> Option<Vec<u8>> {
    cover_for_presentation(source, credentials, key, width, false).await
}

pub async fn card_cover(source: &OpdsSource, credentials: Option<&OpdsCredentials>, key: CardKey, width: u32) -> Option<Vec<u8>> {
    cover_for_presentation(source, credentials, key, width, true).await
}

async fn cover_for_presentation(source: &OpdsSource, credentials: Option<&OpdsCredentials>, key: CardKey, width: u32, card: bool) -> Option<Vec<u8>> {
    let generation = COVER_GENERATION.load(Ordering::SeqCst);
    if let Some(cover) = COVERS.lock().ok()?.get(&(key, width, card)) {
        return cover.clone();
    }
    let href = listed(key, |listed| listed.cover.clone())??;
    let root = Url::parse(&source.url).ok()?;
    let bytes = fetch::bytes(&href, &Access::new(&root, credentials)).await.ok()?;
    let cover = thumbnail_jpeg(&bytes, width, card);
    let mut covers = COVERS.lock().ok()?;
    if COVER_GENERATION.load(Ordering::SeqCst) == generation {
        covers.insert((key, width, card), cover.clone());
    }
    cover
}

fn thumbnail_jpeg(bytes: &[u8], width: u32, card: bool) -> Option<Vec<u8>> {
    let format = image::guess_format(bytes).ok()?;
    let decoded = thumbnail::decode_cover_image(bytes, format).ok()?;
    if card { thumbnail::encode_card_thumbnail_at_width(&decoded, width).ok() } else { thumbnail::encode_thumbnail_at_width(&decoded, width).ok() }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod eviction_tests {
    use super::*;
    use library_model::OpdsSourceId;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::{sync::mpsc, thread};
    use tokio::sync::oneshot;

    #[tokio::test]
    async fn reader_cover_eviction_discards_stale_jpegs_and_preserves_feed_details() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let source = OpdsSource { id: OpdsSourceId::from_u128(94), name: "Fixture".into(), url: origin.clone() };
        let key = CardKey([94; 32]);
        LISTED.lock().unwrap().insert(key, Listed { file_stem: "Fixture".into(), download: None, buy: None, cover: Some(Url::parse(&format!("{origin}/cover.jpg")).unwrap()), description: "Keep this description".into() });
        let jpeg = |red| {
            let pixels = image::RgbImage::from_pixel(2, 3, image::Rgb([red, 0, 0]));
            let mut bytes = Vec::new();
            image::codecs::jpeg::JpegEncoder::new(&mut bytes).encode_image(&pixels).unwrap();
            bytes
        };
        let old = jpeg(0);
        let fresh = jpeg(255);
        let expected = thumbnail_jpeg(&fresh, 2, true).unwrap();
        let (started, received) = oneshot::channel();
        let (resume, wait) = mpsc::channel();
        let server = thread::spawn(move || {
            let mut started = Some(started);
            for body in [old, fresh.clone(), fresh] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut buffer = [0; 1024];
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let read = stream.read(&mut buffer).unwrap();
                    assert_ne!(read, 0);
                    request.extend_from_slice(&buffer[..read]);
                }
                if let Some(started) = started.take() {
                    started.send(()).unwrap();
                    wait.recv().unwrap();
                }
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        let (old, ()) = tokio::join!(card_cover(&source, None, key, 2), async {
            received.await.unwrap();
            evict_cover_cache();
            resume.send(()).unwrap();
        });
        assert_ne!(old.as_deref(), Some(expected.as_slice()));
        assert_eq!(card_cover(&source, None, key, 2).await.as_deref(), Some(expected.as_slice()));
        assert_eq!(card_cover(&source, None, key, 2).await.as_deref(), Some(expected.as_slice()));
        evict_cover_cache();
        assert_eq!(description(key).unwrap(), "Keep this description");
        assert_eq!(card_cover(&source, None, key, 2).await.as_deref(), Some(expected.as_slice()));
        server.join().unwrap();
    }
}
