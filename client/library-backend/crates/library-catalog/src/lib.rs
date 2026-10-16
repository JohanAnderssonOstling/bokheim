//! Catalog files on their origin: hosted books checked against their hashes,
//! and retained cover images. Downloads need no account.
use client_platform_runtime::transfers;
use content_address::ContentHash;
use futures_util::StreamExt;
use library_model::{CardKey, CatalogImportProgress};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use web_time::Instant;

#[cfg(test)]
mod download_tests;

/// Covers fetched this session, by book and size; `None` when the catalog
/// has none. Catalog pages ask for the same covers as they scroll back.
static COVERS: OnceLock<Mutex<HashMap<(CardKey, bool), Option<Vec<u8>>>>> = OnceLock::new();
static CARD_COVERS: OnceLock<Mutex<HashMap<(CardKey, u32), Option<Vec<u8>>>>> = OnceLock::new();
static COVER_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Release retained JPEGs when entering a reader. Loads started before this
/// call may return to their caller, but cannot repopulate either cache.
pub fn evict_cover_caches() {
    COVER_GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Some(covers) = COVERS.get() {
        covers.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
    }
    if let Some(covers) = CARD_COVERS.get() {
        covers.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clear();
    }
}

/// Card assets are generated separately from the retained detail image. An
/// older origin is upgraded once in the CPU worker, then cached by density.
pub async fn card_cover(key: CardKey, width: u32, cpu: &dyn cpu_host::CpuHost) -> Option<Vec<u8>> {
    let generation = COVER_GENERATION.load(Ordering::SeqCst);
    if ![thumbnail::BROWSE_THUMBNAIL_WIDTH, thumbnail::THUMBNAIL_WIDTH].contains(&width) {
        return None;
    }
    let cache = CARD_COVERS.get_or_init(Default::default);
    if let Some(cover) = cache.lock().ok()?.get(&(key, width)) {
        return cover.clone();
    }
    if let Some(bytes) = fetch_cover_asset(key, &thumbnail::card_thumbnail_suffix(width)).await.ok()? {
        let mut cache = cache.lock().ok()?;
        if COVER_GENERATION.load(Ordering::SeqCst) == generation {
            cache.insert((key, width), Some(bytes.clone()));
        }
        return Some(bytes);
    }
    let Some(original) = fetch_cover_at(key, true, generation).await else {
        let mut cache = cache.lock().ok()?;
        if COVER_GENERATION.load(Ordering::SeqCst) == generation {
            cache.insert((key, width), None);
        }
        return None;
    };
    let versions = cpu_host::cover(cpu, original).await.ok()?;
    let selected = if width == thumbnail::BROWSE_THUMBNAIL_WIDTH { versions.browse.clone() } else { versions.card_high_density.clone() };
    let mut cache = cache.lock().ok()?;
    if COVER_GENERATION.load(Ordering::SeqCst) == generation {
        cache.insert((key, thumbnail::BROWSE_THUMBNAIL_WIDTH), Some(versions.browse));
        cache.insert((key, thumbnail::THUMBNAIL_WIDTH), Some(versions.card_high_density));
    }
    Some(selected)
}

/// Where the catalog's files are served. `BOKHEIM_CATALOG_ORIGIN` points a
/// native build at another, for development before the catalog is hosted.
fn origin() -> String {
    std::env::var("BOKHEIM_CATALOG_ORIGIN").unwrap_or_else(|_| "https://catalogs.bokheim.se".into())
}

fn in_catalog(hash: &ContentHash) -> bool {
    library_database::catalog::contains(CardKey(*hash.as_bytes()), Some(false)).unwrap_or(false)
}

/// The catalog book's file, after checking it is the book the catalog names.
pub async fn download_book(hash: ContentHash) -> Result<Vec<u8>, String> {
    download_book_with_progress(hash, &|_| {}).await
}

pub async fn download_book_with_progress(hash: ContentHash, progress: &impl Fn(CatalogImportProgress)) -> Result<Vec<u8>, String> {
    if !in_catalog(&hash) {
        return Err("the book is not in the catalog".into());
    }
    let url = format!("{}/books/{hash}.epub", origin());
    let bytes = download_with_progress(&url, progress).await?;
    progress(CatalogImportProgress::Importing);
    if content_address::hash_reader(&mut bytes.as_slice()).map_err(|error| error.to_string())? != hash {
        return Err("the downloaded book differs from the catalog's".into());
    }
    Ok(bytes)
}

/// An external audiobook from the archive its source offers, repacked as the
/// library's MP3 audiobook archive. Track bytes and source artwork are
/// preserved; catalog artwork is fetched separately for thumbnail generation.
/// The URL
/// comes from the catalog, never the caller; the source's bytes carry no
/// catalog hash to check.
pub async fn download_audiobook(entry: CardKey) -> Result<Vec<u8>, String> {
    download_audiobook_with_progress(entry, &|_| {}).await
}

pub async fn download_audiobook_with_progress(entry: CardKey, progress: &impl Fn(CatalogImportProgress)) -> Result<Vec<u8>, String> {
    let url = library_database::catalog::audiobook_url(entry).map_err(|error| error.to_string())?.ok_or("the audiobook is not offered for download")?;
    let bytes = download_with_progress(&url, progress).await?;
    progress(CatalogImportProgress::Importing);
    client_platform_runtime::executor::run_blocking(move || audiobook_archive::repack_zip(std::io::Cursor::new(bytes), None))
        .await
        .map_err(|error| format!("could not prepare the downloaded audiobook: {error}"))?
        .map_err(|error| format!("could not read the downloaded audiobook: {error}"))
}

async fn download_with_progress(url: &str, progress: &impl Fn(CatalogImportProgress)) -> Result<Vec<u8>, String> {
    loop {
        if !transfers::allowed() {
            progress(CatalogImportProgress::Waiting);
        }
        match transfers::run(download_allowed(url, progress)).await {
            Ok(result) => return result,
            Err(_) => progress(CatalogImportProgress::Waiting),
        }
    }
}

async fn download_allowed(url: &str, progress: &impl Fn(CatalogImportProgress)) -> Result<Vec<u8>, String> {
    progress(CatalogImportProgress::Downloading { downloaded_bytes: 0, total_bytes: None });
    let response = reqwest::get(url).await.and_then(|response| response.error_for_status()).map_err(|error| format!("could not download the book: {error}"))?;
    let total_bytes = response.content_length().filter(|size| *size > 0);
    progress(CatalogImportProgress::Downloading { downloaded_bytes: 0, total_bytes });
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    let mut last_report = Instant::now();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("could not download the book: {error}"))?;
        bytes.extend_from_slice(&chunk);
        if last_report.elapsed() >= Duration::from_millis(100) {
            progress(CatalogImportProgress::Downloading { downloaded_bytes: bytes.len() as u64, total_bytes });
            last_report = Instant::now();
        }
    }
    progress(CatalogImportProgress::Downloading { downloaded_bytes: bytes.len() as u64, total_bytes });
    Ok(bytes)
}

/// Fetch a cover directly by book hash, independently of catalog membership.
/// A failed fetch is not remembered, so the next request tries again.
pub async fn cover(hash: ContentHash, high_density: bool) -> Option<Vec<u8>> {
    fetch_cover(CardKey(*hash.as_bytes()), high_density).await
}

/// An external entry's retained image. The entry identity is not a book hash
/// and cannot authorize a book download.
pub async fn external_cover(entry: CardKey, high_density: bool) -> Option<Vec<u8>> {
    fetch_cover(entry, high_density).await
}

/// Fetch current catalog artwork for an explicit thumbnail replacement.
/// Bypass the browsing cache without modifying the audiobook archive.
pub async fn fresh_external_cover(entry: CardKey) -> Result<Option<Vec<u8>>, String> {
    let generation = COVER_GENERATION.load(Ordering::SeqCst);
    let cover = fetch_origin_cover(entry, true).await?;
    let mut covers = COVERS.get_or_init(Default::default).lock().map_err(|_| "cover cache is unavailable")?;
    if COVER_GENERATION.load(Ordering::SeqCst) == generation {
        covers.insert((entry, true), cover.clone());
    }
    drop(covers);
    CARD_COVERS.get_or_init(Default::default).lock().map_err(|_| "cover cache is unavailable")?.retain(|(key, _), _| *key != entry);
    Ok(cover)
}

async fn fetch_cover(key: CardKey, high_density: bool) -> Option<Vec<u8>> {
    fetch_cover_at(key, high_density, COVER_GENERATION.load(Ordering::SeqCst)).await
}

async fn fetch_cover_at(key: CardKey, high_density: bool, generation: u64) -> Option<Vec<u8>> {
    let covers = COVERS.get_or_init(Default::default);
    if let Some(cover) = covers.lock().ok()?.get(&(key, high_density)) {
        return cover.clone();
    }
    let cover = fetch_origin_cover(key, high_density).await.ok()?;
    let mut covers = covers.lock().ok()?;
    if COVER_GENERATION.load(Ordering::SeqCst) == generation {
        covers.insert((key, high_density), cover.clone());
    }
    cover
}

async fn fetch_origin_cover(key: CardKey, high_density: bool) -> Result<Option<Vec<u8>>, String> {
    fetch_cover_asset(key, if high_density { "detail" } else { "card" }).await
}

async fn fetch_cover_asset(key: CardKey, suffix: &str) -> Result<Option<Vec<u8>>, String> {
    transfers::retry(|| fetch_cover_asset_allowed(key, suffix)).await
}

async fn fetch_cover_asset_allowed(key: CardKey, suffix: &str) -> Result<Option<Vec<u8>>, String> {
    let url = format!("{}/covers/{key}-{suffix}.jpg", origin());
    let request = reqwest::Client::new().get(&url);
    // A manually supplied Cache-Control header triggers a CORS preflight.
    // Let browser Fetch perform cache revalidation without that custom header.
    #[cfg(target_arch = "wasm32")]
    let request = request.fetch_cache_no_cache();
    #[cfg(not(target_arch = "wasm32"))]
    let request = request.header(reqwest::header::CACHE_CONTROL, "no-cache");
    let response = request.send().await.map_err(|error| format!("could not download the cover: {error}"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = response.error_for_status().map_err(|error| format!("could not download the cover: {error}"))?;
    Ok(Some(response.bytes().await.map_err(|error| format!("could not download the cover: {error}"))?.to_vec()))
}
