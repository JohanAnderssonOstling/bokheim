//! The public-domain catalog's files on its origin: book files, checked
//! against the catalog's hash, and cover images. Downloads need no account.
use content_address::ContentHash;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Covers fetched this session, by book and size; `None` when the catalog
/// has none. Catalog pages ask for the same covers as they scroll back.
static COVERS: OnceLock<Mutex<HashMap<(ContentHash, bool), Option<Vec<u8>>>>> = OnceLock::new();

/// Where the catalog's files are served. `BOKHEIM_CATALOG_ORIGIN` points a
/// native build at another, for development before the catalog is hosted.
fn origin() -> String {
    std::env::var("BOKHEIM_CATALOG_ORIGIN").unwrap_or_else(|_| "https://catalog.bokheim.se".into())
}

fn in_catalog(hash: &ContentHash) -> bool {
    library_database::catalog::books().iter().any(|book| book.hash == *hash)
}

/// The catalog book's file, after checking it is the book the catalog names.
pub async fn download_book(hash: ContentHash) -> Result<Vec<u8>, String> {
    if !in_catalog(&hash) {
        return Err("the book is not in the catalog".into());
    }
    let url = format!("{}/books/{hash}.epub", origin());
    let response = reqwest::get(&url).await.and_then(|response| response.error_for_status()).map_err(|error| format!("could not download the book: {error}"))?;
    let bytes = response.bytes().await.map_err(|error| format!("could not download the book: {error}"))?.to_vec();
    if content_address::hash_reader(&mut bytes.as_slice()).map_err(|error| error.to_string())? != hash {
        return Err("the downloaded book differs from the catalog's".into());
    }
    Ok(bytes)
}

/// The catalog's cover for a book, at the card size or the high-density one,
/// or none when the book is not in the catalog or has no cover. A failed
/// fetch is not remembered, so the next request tries again.
pub async fn cover(hash: ContentHash, high_density: bool) -> Option<Vec<u8>> {
    if !in_catalog(&hash) {
        return None;
    }
    let covers = COVERS.get_or_init(Default::default);
    if let Some(cover) = covers.lock().ok()?.get(&(hash, high_density)) {
        return cover.clone();
    }
    let url = format!("{}/covers/{hash}-{}.jpg", origin(), if high_density { "detail" } else { "card" });
    let response = reqwest::get(&url).await.ok()?;
    let cover = match response.status() {
        reqwest::StatusCode::NOT_FOUND => None,
        status if status.is_success() => Some(response.bytes().await.ok()?.to_vec()),
        _ => return None,
    };
    covers.lock().ok()?.insert((hash, high_density), cover.clone());
    cover
}
