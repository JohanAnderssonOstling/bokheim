//! Resolved artwork persisted separately from the read-only metadata snapshot.
use crate::error;
use crate::{MetadataError, MetadataService};
use futures_util::StreamExt;
use rusqlite::OptionalExtension;
use std::time::Duration;

// Shared across services and libraries. Holding the lock also coalesces misses.
static OPEN_LIBRARY: tokio::sync::Mutex<Option<tokio::time::Instant>> = tokio::sync::Mutex::const_new(None);
const MAX_BYTES: usize = 4 * 1024 * 1024;

impl MetadataService {
    pub(crate) fn cached_cover(&self, id: &str) -> Result<Option<Vec<u8>>, MetadataError> {
        self.state.demand.lock().map_err(error)?.query_row("SELECT bytes FROM resolved_cover WHERE id=?1", [id], |r| r.get(0)).optional().map_err(error)
    }

    fn save_cover(&self, id: &str, bytes: Vec<u8>) -> Result<Option<metadata_contract::books::CoverSource>, MetadataError> {
        if bytes.is_empty() || bytes.len() > MAX_BYTES {
            return Err(MetadataError("invalid artwork size".into()));
        }
        self.state.demand.lock().map_err(error)?.execute("INSERT OR IGNORE INTO resolved_cover VALUES(?1,?2)", rusqlite::params![id, bytes]).map_err(error)?;
        Ok(Some(metadata_contract::books::CoverSource { id: id.to_owned() }))
    }

    pub(crate) async fn resolve_audible_cover(&self, asin: &str, region: &str) -> Result<Option<metadata_contract::books::CoverSource>, MetadataError> {
        let id = format!("audible-{region}-{asin}");
        if self.cached_cover(&id)?.is_some() {
            return Ok(Some(metadata_contract::books::CoverSource { id }));
        }
        let bytes = crate::serve::audible::client().cover(region, asin).await.map_err(MetadataError)?;
        bytes.map(|bytes| self.save_cover(&id, bytes)).transpose().map(Option::flatten)
    }

    pub(crate) async fn resolve_isbn_cover(&self, isbns: &[String]) -> Result<Option<metadata_contract::books::CoverSource>, MetadataError> {
        self.resolve_isbn_cover_at(isbns, "https://covers.openlibrary.org").await
    }

    pub(crate) async fn resolve_isbn_cover_at(&self, isbns: &[String], origin: &str) -> Result<Option<metadata_contract::books::CoverSource>, MetadataError> {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(30)).user_agent("Bokheim/0.1 (cover enrichment)").build().map_err(error)?;
        for isbn in isbns {
            if book_model::canonical_isbn(isbn).as_deref() != Some(isbn.as_str()) {
                return Err(MetadataError("invalid cover ISBN".into()));
            }
            let id = format!("isbn-{isbn}");
            let mut last = OPEN_LIBRARY.lock().await;
            if self.cached_cover(&id)?.is_some() {
                return Ok(Some(metadata_contract::books::CoverSource { id }));
            }
            if let Some(previous) = *last {
                tokio::time::sleep(Duration::from_secs(1).saturating_sub(previous.elapsed())).await;
            }
            *last = Some(tokio::time::Instant::now());
            let response = client.get(format!("{origin}/b/isbn/{isbn}-L.jpg?default=false")).send().await.map_err(error)?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                continue;
            }
            let response = response.error_for_status().map_err(error)?;
            if response.content_length().is_some_and(|n| n > MAX_BYTES as u64) {
                return Err(MetadataError("artwork is too large".into()));
            }
            let mut bytes = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(error)?;
                if bytes.len() + chunk.len() > MAX_BYTES {
                    return Err(MetadataError("artwork is too large".into()));
                }
                bytes.extend_from_slice(&chunk);
            }
            return self.save_cover(&id, bytes);
        }
        Ok(None)
    }
}
