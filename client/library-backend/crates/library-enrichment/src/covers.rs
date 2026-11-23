//! Missing cover art downloaded directly from external providers. Covers
//! are set once: art only fills a book that has none.

use book_model::BookRecord;
use cpu_contract::{cover, ThumbnailBytes};
use library_model::BlobKind;
use std::time::Duration;
use sync_common::BookId;

use crate::{EnrichmentResult, LibraryEnrichment};

impl LibraryEnrichment {
    /// Look up pending covers in bounded pages. Artwork publication owns its notifications.
    pub(crate) async fn fill_covers(&self) -> EnrichmentResult<()> {
        let mut after = None;
        loop {
            let candidates = self.database.enrichment_cover_candidates(after.as_ref()).await?;
            let Some(last) = candidates.last() else { break };
            after = Some(*last);
            for hash in candidates {
                if let Err(error) = self.fill_cover(hash).await {
                    log::warn!("cover for {hash} will retry: {error}");
                    self.events.enrichment.wake_after(Duration::from_secs(60));
                }
            }
        }
        Ok(())
    }

    async fn fill_cover(&self, hash: BookId) -> EnrichmentResult<()> {
        let Some((record, expected, body)) = self.database.enrichment_cover_input(&hash).await? else { return Ok(()) };
        let assets = self.state.read().await.assets().clone();
        if assets.read_bytes(BlobKind::Thumbnail, &hash).await?.is_none() {
            if let Some(record) = record {
                if self.fetch_enriched_cover(hash, &record, &expected).await?.is_none() {
                    self.events.enrichment.wake();
                    return Ok(());
                }
            }
        }
        self.database.mark_enrichment_cover_checked_if_current(&hash, body.as_deref()).await?;
        Ok(())
    }

    async fn fetch_enriched_cover(&self, hash: BookId, record: &BookRecord, expected: &Option<String>) -> EnrichmentResult<Option<bool>> {
        let bytes = self.covers.book_cover(record).await?;
        let versions = match bytes {
            Some(bytes) => Some(cover(&*self.cpu, bytes).await.map_err(|error| error.to_string())?),
            None => None,
        };
        self.apply_enriched_cover(hash, expected, versions.as_ref()).await
    }

    /// Stores fetched art only if the book still has no cover and its
    /// thumbnail work is unchanged since the fetch began. None means it changed.
    pub(crate) async fn apply_enriched_cover(&self, hash: BookId, expected: &Option<String>, versions: Option<&ThumbnailBytes>) -> EnrichmentResult<Option<bool>> {
        let storage = self.state.read().await.clone();
        if self.database.thumbnail_work_state(&hash).await? != *expected {
            return Ok(None);
        }
        let Some(versions) = versions else {
            return Ok(storage.assets().read_bytes(BlobKind::Thumbnail, &hash).await?.is_none().then_some(false));
        };
        if !storage.assets().write_missing_thumbnail_set(&hash, versions).await? {
            return Ok(None);
        }
        self.events.cover_changed(hash);
        Ok(self.database.complete_thumbnail_if_current(&hash, expected.as_deref()).await?.then_some(true))
    }
}
