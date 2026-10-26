//! Missing cover art downloaded directly from external providers. Covers
//! are set once: art only fills a book that has none.

use book_model::BookRecord;
use library_runtime::events::LibraryEvent;
use std::time::Duration;
use sync_common::BookId;

use crate::{EnrichmentResult, LibraryEnrichment};

impl LibraryEnrichment {
    /// Looks up the cover of every answered book in `scope` that has none.
    /// Returns how many covers were filled.
    pub(crate) async fn fill_covers(&self, scope: &[BookId]) -> EnrichmentResult<usize> {
        if !self.assets.local_access_enabled() { return Ok(0); }
        let mut filled = 0;
        for hash in self.database.enrichment_cover_candidates(scope)? {
            // Do not spend provider requests when another device or worker
            // has already supplied local artwork.
            if self.assets.read_bytes(library_model::BlobKind::Thumbnail, &hash).await?.is_some() {
                self.database.mark_enrichment_cover_checked(&hash)?;
                continue;
            }
            let Some(record) = self.database.enrichment_cover_record(&hash)? else {
                self.database.mark_enrichment_cover_checked(&hash)?;
                continue;
            };
            match self.fetch_enriched_cover(hash, &record).await {
                Ok(Some(found)) => {
                    filled += usize::from(found);
                    self.database.mark_enrichment_cover_checked(&hash)?;
                }
                // The book changed meanwhile; look again on the next pass.
                Ok(None) => {}
                Err(error) => {
                    let title = self.database.browse_book_detail(&hash).ok().map(|detail| detail.book.title).unwrap_or_else(|| "Untitled".into());
                    log::warn!("cover for {title:?} will retry: {error}");
                    self.events.enrichment.wake_after(Duration::from_secs(60));
                }
            }
        }
        Ok(filled)
    }

    async fn fetch_enriched_cover(&self, hash: BookId, record: &BookRecord) -> EnrichmentResult<Option<bool>> {
        let expected = self.database.thumbnail_work_state(&hash)?;
        let bytes = self.covers.book_cover(record).await?;
        let versions = match bytes {
            Some(bytes) => Some(cpu_host::cover(&*self.cpu, bytes).await.map_err(|error| error.to_string())?),
            None => None,
        };
        self.apply_enriched_cover(hash, &expected, versions.as_ref()).await
    }

    /// Stores fetched art only if the book still has no cover and its
    /// thumbnail work is unchanged since the fetch began. None means it changed.
    pub(crate) async fn apply_enriched_cover(&self, hash: BookId, expected: &Option<String>, versions: Option<&cpu_host::ThumbnailBytes>) -> EnrichmentResult<Option<bool>> {
        let _lease = self.assets.lease_book(&hash).await?;
        if self.assets.read_bytes(library_model::BlobKind::Thumbnail, &hash).await?.is_some() || self.database.thumbnail_work_state(&hash)? != *expected {
            return Ok(None);
        }
        let Some(versions) = versions else { return Ok(Some(false)) };
        drop(_lease);
        asset_transfer::thumbnail_jobs::store_thumbnail(&self.assets, hash, &versions.browse, &versions.card_high_density, &versions.high_density).await?;
        self.events.emit(LibraryEvent::ThumbnailGenerated { content_hash: hash });
        if self.database.thumbnail_work_state(&hash)? != *expected {
            return Ok(None);
        }
        self.database.complete_thumbnail_work(&hash, true)?;
        Ok(Some(true))
    }
}
