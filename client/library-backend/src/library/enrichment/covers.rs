//! Missing cover art from where the metadata server's answer points. Covers
//! are set once: art only fills a book that has none.
pub(crate) use crate::library::LibrarySession;
pub(crate) use crate::BackendError;
pub(crate) use crate::ContentHash;
pub(crate) use metadata_contract::books::CoverSource;

impl LibrarySession {
    /// Fetches the answered cover of every book in `scope` that has none.
    /// Returns how many covers were filled.
    pub(crate) async fn fill_covers(&self, scope: &[ContentHash]) -> Result<usize, BackendError> {
        let mut filled = 0;
        for hash in self.db.enrichment_cover_candidates(scope)? {
            let Some(source) = self.db.enrichment_cover_source(&hash)? else {
                self.db.mark_enrichment_cover_checked(&hash)?;
                continue;
            };
            match self.fetch_enriched_cover(hash, &source).await {
                Ok(Some(found)) => {
                    filled += usize::from(found);
                    self.db.mark_enrichment_cover_checked(&hash)?;
                }
                // The book changed meanwhile; look again on the next pass.
                Ok(None) => {}
                Err(error) => log::warn!("cover for {hash} will retry: {error}"),
            }
        }
        Ok(filled)
    }

    async fn fetch_enriched_cover(&self, hash: ContentHash, source: &CoverSource) -> Result<Option<bool>, BackendError> {
        let expected = self.db.thumbnail_work_state(&hash)?;
        let bytes = self.metadata.resolved_cover(&source.id).await.map_err(BackendError::operation)?;
        let versions = match bytes {
            Some(bytes) => Some(cpu_host::cover(&*self.cpu, bytes).await.map_err(BackendError::operation)?),
            None => None,
        };
        self.apply_enriched_cover(hash, &expected, versions.as_ref()).await
    }

    /// Stores fetched art only if the book still has no cover and its
    /// thumbnail work is unchanged since the fetch began. None means it changed.
    pub(crate) async fn apply_enriched_cover(&self, hash: ContentHash, expected: &Option<(String, i64)>, versions: Option<&cpu_host::ThumbnailBytes>) -> Result<Option<bool>, BackendError> {
        let _lease = self.assets.lease_book(&hash).await.map_err(BackendError::operation)?;
        if self.assets.read_bytes(library_model::BlobKind::Thumbnail, &hash).await.map_err(BackendError::operation)?.is_some() || self.db.thumbnail_work_state(&hash)? != *expected {
            return Ok(None);
        }
        let Some(versions) = versions else { return Ok(Some(false)) };
        self.store_thumbnail(hash, versions).await?;
        if self.db.thumbnail_work_state(&hash)? != *expected {
            return Ok(None);
        }
        self.db.complete_thumbnail_work(&hash, true).map_err(BackendError::operation)?;
        Ok(Some(true))
    }
}
