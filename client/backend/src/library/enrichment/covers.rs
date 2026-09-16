//! Cover acquisition and replacement.
use super::OPEN_LIBRARY_COVER_PROVIDER;
use super::application::{ENRICHMENT_WRITE_BATCH_SIZE, EnrichmentWrite, apply_enrichment_writes};
use crate::BackendError;
use crate::ContentHash;
use crate::library::LibrarySession;
use client_library::prepared_enrichment as prepared;

impl LibrarySession {
    /// Cover enrichment consumes identifiers already persisted by ISBN enrichment.
    pub(super) async fn enrich_cover_candidates(&self, candidates: Vec<(ContentHash, Vec<String>)>) -> Result<usize, BackendError> {
        let mut updated = 0;
        let mut writes: Vec<EnrichmentWrite> = Vec::new();
        for (hash, isbns) in candidates {
            let _lease = self.assets.lease_book(&hash).await.map_err(BackendError::operation)?;
            let state_hash = hash.clone();
            let (state, format) = self
                .db
                .command(crate::storage::storage_queue::Priority::Background, move |conn| Ok((client_library::thumbnail_jobs::thumbnail_work_state(conn, &state_hash)?, client_library::assets::get_book_format(conn, &state_hash)?)))
                .await
                .map_err(BackendError::operation)?;
            let Some((state, _)) = state else { continue };
            if state != "no_cover" && state != "ready" {
                continue;
            }
            let mut complete = false;
            let outcome: Result<bool, BackendError> = async {
                let existing = self.assets.read_bytes(crate::BlobKind::Thumbnail, &hash).await?;
                if existing.is_some() && state == "no_cover" {
                    complete = true;
                }
                let replacing = if let Some(bytes) = existing {
                    let (width, height) = thumbnail::cover_image_dimensions(&bytes).map_err(BackendError::operation)?;
                    // Near-square and landscape audiobook art can be upgraded;
                    // leave portrait covers and other publication formats alone.
                    if format != Some(library_model::BookFormat::M4b) || f64::from(width) / f64::from(height) < 0.9 {
                        return Ok(false);
                    }
                    true
                } else {
                    false
                };
                for isbn in &isbns {
                    let result = self.metadata().cover(isbn).await;
                    let Some(bytes) = result.map_err(BackendError::operation)? else { continue };
                    if replacing {
                        let (width, height) = thumbnail::cover_image_dimensions(&bytes).map_err(BackendError::operation)?;
                        if !thumbnail::reasonable_cover_dimensions(width, height) || f64::from(width) / f64::from(height) >= 0.9 {
                            continue;
                        }
                    }
                    let versions = crate::worker_cpu::cover(bytes).await.map_err(BackendError::operation)?;
                    self.store_thumbnail(hash, &versions).await?;
                    complete = true;
                    return Ok(true);
                }
                Ok(false)
            }
            .await;
            let (status, detail) = match outcome {
                Ok(true) => {
                    updated += 1;
                    ("updated", None)
                }
                Ok(false) => ("no_match", None),
                Err(error) => {
                    log::warn!("cover enrichment for {hash} will retry: {error}");
                    ("failed", Some(error.to_string()))
                }
            };
            writes.push(Box::new(move |conn| {
                if complete {
                    client_library::thumbnail_jobs::complete_thumbnail_work(conn, &hash, true)?;
                }
                prepared::record_external_metadata_attempt(conn, &hash, OPEN_LIBRARY_COVER_PROVIDER, &isbns.join(","), status, detail.as_deref())?;
                Ok(0)
            }));
            if writes.len() == ENRICHMENT_WRITE_BATCH_SIZE {
                apply_enrichment_writes(self, std::mem::take(&mut writes)).await?;
            }
        }
        apply_enrichment_writes(self, writes).await?;
        Ok(updated)
    }
}
