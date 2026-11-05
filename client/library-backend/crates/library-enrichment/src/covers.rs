//! Missing cover art downloaded directly from external providers. Covers
//! are set once: art only fills a book that has none.

use book_model::BookRecord;
use metadata_contract::books::MAX_BOOKS_PER_REQUEST;
use std::time::Duration;
use sync_common::BookId;

use crate::{EnrichmentResult, LibraryEnrichment};

impl LibraryEnrichment {
    /// Looks up the cover of every answered book in `scope` that has none.
    /// Returns how many covers were filled.
    pub(crate) async fn fill_covers(&self, scope: &[BookId]) -> EnrichmentResult<usize> {
        let assets = self.state.read().await.assets().clone();
        let mut filled = 0;
        let candidates = self.database.enrichment_cover_candidates(scope).await?;
        for batch in candidates.chunks(MAX_BOOKS_PER_REQUEST) {
            let mut checked = Vec::new();
            let mut completed = Vec::new();
            let result: EnrichmentResult<()> = async {
                let mut requested = Vec::new();
                for hash in batch {
                    // Do not spend provider requests when another device or worker
                    // has already supplied local artwork.
                    if assets.read_bytes(library_model::BlobKind::Thumbnail, hash).await?.is_some() {
                        checked.push(*hash);
                        continue;
                    }
                    requested.push(*hash);
                }
                if requested.is_empty() {
                    return Ok(());
                }
                let inputs = self.database.enrichment_cover_inputs(&requested).await?;
                for (hash, record, expected, publication) in inputs {
                    let Some(record) = record else {
                        completed.push((hash, publication));
                        continue;
                    };
                    match self.fetch_enriched_cover(hash, &record, &expected).await {
                        Ok(Some(found)) => {
                            filled += usize::from(found);
                            completed.push((hash, publication));
                        }
                        // The book changed meanwhile; look again on the next pass.
                        Ok(None) => {}
                        Err(error) => {
                            let title = if record.metadata.title.is_empty() { "Untitled" } else { &record.metadata.title };
                            log::warn!("cover for {title:?} will retry: {error}");
                            self.events.enrichment.wake_after(Duration::from_secs(60));
                        }
                    }
                }
                Ok(())
            }
            .await;
            // Keep completed progress even when a later file/provider operation
            // in this bounded batch fails.
            self.database.mark_enrichment_covers_checked(&checked).await?;
            self.database.mark_enrichment_covers_checked_if_current(&completed).await?;
            result?;
        }
        Ok(filled)
    }

    async fn fetch_enriched_cover(&self, hash: BookId, record: &BookRecord, expected: &Option<String>) -> EnrichmentResult<Option<bool>> {
        let bytes = self.covers.book_cover(record).await?;
        let versions = match bytes {
            Some(bytes) => Some(cpu_host::cover(&*self.cpu, bytes).await.map_err(|error| error.to_string())?),
            None => None,
        };
        self.apply_enriched_cover(hash, expected, versions.as_ref()).await
    }

    /// Stores fetched art only if the book still has no cover and its
    /// thumbnail work is unchanged since the fetch began. None means it changed.
    pub(crate) async fn apply_enriched_cover(&self, hash: BookId, expected: &Option<String>, versions: Option<&cpu_host::ThumbnailBytes>) -> EnrichmentResult<Option<bool>> {
        let storage = self.state.read().await.clone();
        let _lease = storage.assets().lease_book(&hash).await?;
        if storage.assets().read_bytes(library_model::BlobKind::Thumbnail, &hash).await?.is_some() || self.database.thumbnail_work_state(&hash).await? != *expected {
            return Ok(None);
        }
        let Some(versions) = versions else { return Ok(Some(false)) };
        drop(_lease);
        asset_transfer::thumbnail_jobs::store_thumbnail(storage.assets(), hash, &versions.browse, &versions.card_high_density, &versions.high_density).await?;
        self.events.cover_changed(hash);
        Ok(self.database.complete_thumbnail_if_current(&hash, expected.as_deref()).await?.then_some(true))
    }
}
