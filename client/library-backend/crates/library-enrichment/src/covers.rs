//! Missing cover art from where the metadata server's answer points. Covers
//! are set once: art only fills a book that has none.

use library_runtime::events::LibraryEvent;
use metadata_contract::books::CoverSource;
use sync_common::ContentHash;

use crate::{EnrichmentResult, LibraryEnrichment};

impl LibraryEnrichment {
    /// Fetches the answered cover of every book in `scope` that has none.
    /// Returns how many covers were filled.
    pub(crate) async fn fill_covers(&self, scope: &[ContentHash]) -> EnrichmentResult<usize> {
        let mut filled = 0;
        for hash in self.database.enrichment_cover_candidates(scope)? {
            let Some(source) = self.database.enrichment_cover_source(&hash)? else {
                self.database.mark_enrichment_cover_checked(&hash)?;
                continue;
            };
            match self.fetch_enriched_cover(hash, &source).await {
                Ok(Some(found)) => {
                    filled += usize::from(found);
                    self.database.mark_enrichment_cover_checked(&hash)?;
                }
                // The book changed meanwhile; look again on the next pass.
                Ok(None) => {}
                Err(error) => {
                    let title = self.database.browse_book_detail(&hash).ok().map(|detail| detail.book.title).unwrap_or_else(|| "Untitled".into());
                    log::warn!("cover for {title:?} will retry: {error}");
                }
            }
        }
        Ok(filled)
    }

    async fn fetch_enriched_cover(&self, hash: ContentHash, source: &CoverSource) -> EnrichmentResult<Option<bool>> {
        let expected = self.database.thumbnail_work_state(&hash)?;
        let bytes = self.client.resolved_cover(&source.id).await?;
        let versions = match bytes {
            Some(bytes) => Some(cpu_host::cover(&*self.cpu, bytes).await.map_err(|error| error.to_string())?),
            None => None,
        };
        self.apply_enriched_cover(hash, &expected, versions.as_ref()).await
    }

    /// Stores fetched art only if the book still has no cover and its
    /// thumbnail work is unchanged since the fetch began. None means it changed.
    async fn apply_enriched_cover(&self, hash: ContentHash, expected: &Option<String>, versions: Option<&cpu_host::ThumbnailBytes>) -> EnrichmentResult<Option<bool>> {
        let _lease = self.assets.lease_book(&hash).await?;
        if self.assets.read_bytes(library_model::BlobKind::Thumbnail, &hash).await?.is_some() || self.database.thumbnail_work_state(&hash)? != *expected {
            return Ok(None);
        }
        let Some(versions) = versions else { return Ok(Some(false)) };
        asset_transfer::thumbnail_jobs::store_thumbnail(&self.assets, hash, &versions.browse, &versions.high_density).await?;
        self.events.emit(LibraryEvent::ThumbnailGenerated { content_hash: hash });
        if self.database.thumbnail_work_state(&hash)? != *expected {
            return Ok(None);
        }
        self.database.complete_thumbnail_work(&hash, true)?;
        Ok(Some(true))
    }
}
