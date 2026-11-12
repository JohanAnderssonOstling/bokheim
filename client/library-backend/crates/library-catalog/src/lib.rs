//! Shared catalog snapshots, hosted books checked against their hashes,
//! and retained cover images. Downloads need no account.
use client_platform_runtime::{executor::run_blocking, transfers};
use client_protocol::{BackendError, Failure};
use content_address::BookId;
use futures_util::StreamExt;
use library_model::{CardKey, CatalogImportProgress};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use web_time::Instant;

mod cover_cache;
pub use cover_cache::MAX_BYTES as COVER_CACHE_MAX_BYTES;
use cover_cache::{Address, CoverCache};

mod snapshots;
pub use snapshots::start;

#[cfg(test)]
mod download_tests;

/// One cache across libraries and display sizes. Disk retains successful
/// responses; a bounded set remembers missing covers without retaining images.
static COVERS: OnceLock<CoverCache> = OnceLock::new();
static COVER_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Open an app-wide directory of cover files on native storage or browser OPFS.
pub async fn configure_cover_cache(path: &Path, max_bytes: u64) -> std::io::Result<()> {
    COVERS.get_or_init(Default::default).configure(path, max_bytes).await
}

/// Forget missing-cover responses when entering a reader. Loads started before
/// this call may return to their caller, but cannot repopulate the set. Persistent
/// covers remain available when returning to browsing or restarting offline.
pub fn evict_cover_caches() {
    COVER_GENERATION.fetch_add(1, Ordering::SeqCst);
    if let Some(covers) = COVERS.get() {
        covers.clear();
    }
}

/// Cache finished server artwork by density.
pub async fn card_cover(key: CardKey, width: u32) -> Option<Vec<u8>> {
    let generation = COVER_GENERATION.load(Ordering::SeqCst);
    if ![thumbnail::BROWSE_THUMBNAIL_WIDTH, thumbnail::THUMBNAIL_WIDTH].contains(&width) {
        return None;
    }
    fetch_cover_at(key, &thumbnail::card_thumbnail_suffix(width), generation).await.ok().flatten()
}

/// Where the catalog's files are served. `BOKHEIM_CATALOG_ORIGIN` points a
/// native build at another, for development before the catalog is hosted.
fn origin() -> String {
    std::env::var("BOKHEIM_CATALOG_ORIGIN").unwrap_or_else(|_| "https://catalogs.bokheim.se".into())
}

async fn in_catalog(hash: BookId) -> bool {
    run_blocking(move || library_database::catalog::contains(CardKey::from(hash), Some(false)).unwrap_or(false)).await.unwrap_or(false)
}

/// The catalog book's file, after checking it is the book the catalog names.
pub async fn download_book(hash: BookId) -> Result<Vec<u8>, BackendError> {
    download_book_with_progress(hash, &|_| {}).await
}

pub async fn download_book_with_progress(hash: BookId, progress: &impl Fn(CatalogImportProgress)) -> Result<Vec<u8>, BackendError> {
    if !in_catalog(hash).await {
        return Err(BackendError::classified(Failure::NotFound, "the book is not in the catalog"));
    }
    let url = format!("{}/books/{hash}.epub", origin());
    let bytes = download_with_progress(&url, progress).await?;
    progress(CatalogImportProgress::Importing);
    run_blocking(move || {
        if Some(content_address::hash_reader(&mut bytes.as_slice())?) != hash.content_hash() {
            return Err(BackendError::classified(Failure::UnreadableBook, "the downloaded book differs from the catalog's"));
        }
        Ok(bytes)
    }).await.map_err(BackendError::operation)?
}

/// An external audiobook from the archive its source offers, repacked as the
/// library's MP3 audiobook archive. Track bytes and source artwork are
/// preserved; catalog artwork is fetched separately for thumbnail generation.
/// The URL
/// comes from the catalog, never the caller; the source's bytes carry no
/// catalog hash to check.
pub async fn download_audiobook(entry: CardKey) -> Result<Vec<u8>, BackendError> {
    download_audiobook_with_progress(entry, &|_| {}).await
}

pub async fn download_audiobook_with_progress(entry: CardKey, progress: &impl Fn(CatalogImportProgress)) -> Result<Vec<u8>, BackendError> {
    let url = run_blocking(move || library_database::catalog::audiobook_url(entry))
        .await
        .map_err(BackendError::operation)?
        .map_err(BackendError::operation)?
        .ok_or_else(|| BackendError::classified(Failure::NotFound, "the audiobook is not offered for download"))?;
    let bytes = download_with_progress(&url, progress).await?;
    progress(CatalogImportProgress::Importing);
    client_platform_runtime::executor::run_blocking(move || audiobook_archive::repack_zip(std::io::Cursor::new(bytes), None))
        .await
        .map_err(|error| BackendError::message(format!("could not prepare the downloaded audiobook: {error}")))?
        .map_err(|error| BackendError::classified(Failure::UnreadableBook, format!("could not read the downloaded audiobook: {error}")))
}

async fn download_with_progress(url: &str, progress: &impl Fn(CatalogImportProgress)) -> Result<Vec<u8>, BackendError> {
    // Catalog acquisitions are explicit user downloads.
    progress(CatalogImportProgress::Downloading { downloaded_bytes: 0, total_bytes: None });
    let response = http_response::get(url).await.and_then(|response| response.error_for_status()).map_err(|error| BackendError::http("could not download the book", error))?;
    let total_bytes = response.content_length().filter(|size| *size > 0);
    progress(CatalogImportProgress::Downloading { downloaded_bytes: 0, total_bytes });
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    let mut last_report = Instant::now();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| BackendError::http("could not download the book", error))?;
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
pub async fn cover(hash: BookId, high_density: bool) -> Option<Vec<u8>> {
    fetch_cover(CardKey::from(hash), high_density).await
}

/// An external entry's retained image. The entry identity is not a book hash
/// and cannot authorize a book download.
pub async fn external_cover(entry: CardKey, high_density: bool) -> Option<Vec<u8>> {
    fetch_cover(entry, high_density).await
}

/// Fetch selected artwork for a hosted or external catalog import.
/// Prefer the current master, but keep cached catalog artwork usable when
/// the cover server is unavailable. This never modifies the book archive.
pub async fn fresh_catalog_cover(entry: CardKey) -> Result<Option<Vec<u8>>, BackendError> {
    let generation = COVER_GENERATION.load(Ordering::SeqCst);
    let origin = origin();
    let cache = COVERS.get_or_init(Default::default);
    let result = fetch_cover_asset_allowed(&origin, entry, "detail").await;
    if let Ok(Some(cover)) = &result {
        cache.replace((origin, entry, "detail".into()), Some(cover.clone()), generation).await;
        return result;
    }
    // Browsing may have cached only a card. It still preserves the selected
    // artwork better than falling back to a different embedded cover.
    for suffix in ["detail".to_owned(), thumbnail::card_thumbnail_suffix(thumbnail::THUMBNAIL_WIDTH), thumbnail::card_thumbnail_suffix(thumbnail::BROWSE_THUMBNAIL_WIDTH)] {
        if let Some(Some(cover)) = cache.get(&(origin.clone(), entry, suffix)).await {
            return Ok(Some(cover));
        }
    }
    result
}

async fn fetch_cover(key: CardKey, high_density: bool) -> Option<Vec<u8>> {
    let suffix = if high_density { "detail".to_owned() } else { thumbnail::card_thumbnail_suffix(thumbnail::BROWSE_THUMBNAIL_WIDTH) };
    fetch_cover_at(key, &suffix, COVER_GENERATION.load(Ordering::SeqCst)).await.ok().flatten()
}

async fn fetch_cover_at(key: CardKey, suffix: &str, generation: u64) -> Result<Option<Vec<u8>>, BackendError> {
    let address: Address = (origin(), key, suffix.to_owned());
    let cache = COVERS.get_or_init(Default::default);
    if let Some(cover) = cache.get(&address).await {
        return Ok(cover);
    }
    let cover = fetch_cover_asset(&address.0, key, suffix).await?;
    cache.insert(address, cover.clone(), generation).await;
    Ok(cover)
}

async fn fetch_cover_asset(origin: &str, key: CardKey, suffix: &str) -> Result<Option<Vec<u8>>, BackendError> {
    // A paused cover must not occupy the library's bounded reader slots.
    // Failed fetches are not cached; the policy observer refreshes the view
    // when automatic transfers become available again.
    transfers::try_run(fetch_cover_asset_allowed(origin, key, suffix)).await.map_err(BackendError::operation)?
}

async fn fetch_cover_asset_allowed(origin: &str, key: CardKey, suffix: &str) -> Result<Option<Vec<u8>>, BackendError> {
    let url = format!("{origin}/covers/{key}-{suffix}.jpg");
    let request = http_response::client().get(&url);
    // A manually supplied Cache-Control header triggers a CORS preflight.
    // Let browser Fetch perform cache revalidation without that custom header.
    #[cfg(target_arch = "wasm32")]
    let request = request.fetch_cache_no_cache();
    #[cfg(not(target_arch = "wasm32"))]
    let request = request.header(reqwest::header::CACHE_CONTROL, "no-cache");
    let response = request.send().await.map_err(|error| BackendError::operation(error).context("could not download the cover"))?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let response = response.error_for_status().map_err(|error| BackendError::operation(error).context("could not download the cover"))?;
    Ok(Some(response.bytes().await.map_err(|error| BackendError::operation(error).context("could not download the cover"))?.to_vec()))
}
