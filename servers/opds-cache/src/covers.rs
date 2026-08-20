use super::{publication_cover_fallback, publication_metadata_acquisition, CatalogPage, CatalogService, CatalogServiceError, CatalogSourceId, BROWSER_UPSTREAM_TIMEOUT};
use crate::extraction::{generate_publication_thumbnail, is_synthetic_cover_key};
impl CatalogService {
    pub(crate) async fn remember_browser_covers(&self, source_id: &CatalogSourceId, page: &CatalogPage) {
        let publications = page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).collect::<Vec<_>>();
        {
            let mut recent = self.state.recent_browser_covers.lock().await;
            for publication in &publications {
                recent.remember(source_id, publication);
            }
        }
        let urls = publications.into_iter().filter_map(|publication| publication.image_url.clone()).collect::<Vec<_>>();
        self.state.cover_pipeline.enqueue(source_id.clone(), urls.into_iter().filter(|url| !is_synthetic_cover_key(url))).await;
    }

    pub(crate) async fn prepare_publication_cover_fallbacks(&self, source_id: &CatalogSourceId, page: &mut CatalogPage) {
        for publication in page.publications.iter_mut().chain(page.groups.iter_mut().flat_map(|group| group.publications.iter_mut())) {
            let _ = publication_cover_fallback(source_id, publication);
        }
    }
    pub async fn catalog_cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, CatalogServiceError> {
        self.public_endpoint(source_id).await?;
        let cache_source = source_id.clone();
        let cache_image = image_url.to_owned();
        let active = self.cache_call(move |cache| cache.active_publication_for_cover(&cache_source, &cache_image)).await?;
        let recent = self.state.recent_browser_covers.lock().await.get(source_id, image_url);
        if active.is_none() && recent.is_none() {
            return Err(CatalogServiceError("cover URL is not present in the active catalog generation".to_owned()));
        }
        self.state.covers.remember_cover_url(source_id, image_url);
        let publication_identity = active.as_ref().map(|publication| publication.identity.clone()).or_else(|| recent.as_ref().map(|reference| reference.identity.clone()));
        let epub_acquisition = active.as_ref().and_then(publication_metadata_acquisition).or_else(|| recent.as_ref().and_then(|reference| reference.acquisition.clone()));
        let browser_known = recent.is_some();
        if let Some(acquisition) = epub_acquisition {
            let mut upstream_cover_is_unsuitable = false;
            // A real OPDS URL may already have produced a sufficiently large,
            // cover-shaped cached thumbnail. Synthetic keys always represent
            // an EPUB fallback and therefore skip this probe.
            if !is_synthetic_cover_key(image_url) {
                match self.state.covers.cached_cover(image_url).await {
                    Ok(bytes) => return Ok(bytes),
                    Err(error) if error.is_unsuitable_cover_dimensions() => upstream_cover_is_unsuitable = true,
                    Err(_) if browser_known => match self.state.covers.cover(source_id, image_url).await {
                        Ok(bytes) => return Ok(bytes),
                        Err(error) if error.is_unsuitable_cover_dimensions() => upstream_cover_is_unsuitable = true,
                        Err(_) => {}
                    },
                    Err(_) => {}
                }
            }
            // Promote a queued metadata extraction when a reader needs the
            // EPUB fallback. Whichever task claims the extraction downloads
            // the EPUB once; the other waits for the generated JPEG instead of
            // issuing a competing acquisition request.
            if let Some(identity) = publication_identity.as_ref() {
                let cache_source = source_id.clone();
                let cache_url = acquisition.url.clone();
                self.cache_write_call(move |cache| cache.queue_epub_extractions(&cache_source, std::slice::from_ref(&cache_url), false)).await?;
                if let Err(error) = self.enrich_active_publication(source_id, identity.clone(), acquisition.clone(), Some(image_url.to_owned())).await {
                    tracing::warn!(source = source_id.as_str(), %error, "on-demand EPUB cover enrichment failed");
                }
                let deadline = tokio::time::Instant::now() + BROWSER_UPSTREAM_TIMEOUT;
                loop {
                    let notified = self.state.extraction_notify.notified();
                    if let Ok(bytes) = self.state.covers.cached_cover(image_url).await {
                        return Ok(bytes);
                    }
                    let cache_source = source_id.clone();
                    let cache_url = acquisition.url.clone();
                    let status = self.cache_call(move |cache| cache.epub_extraction_status(&cache_source, &cache_url)).await?;
                    let now = tokio::time::Instant::now();
                    if !matches!(status.as_deref(), Some("pending" | "running")) || now >= deadline {
                        break;
                    }
                    let _ = tokio::time::timeout(deadline - now, notified).await;
                }
            }
            let _download_permit = tokio::time::timeout(BROWSER_UPSTREAM_TIMEOUT, self.state.interactive_download_slots.acquire())
                .await
                .map_err(|_| CatalogServiceError("interactive catalog download capacity timed out".to_owned()))?
                .map_err(|error| CatalogServiceError(error.to_string()))?;
            let downloaded = tokio::time::timeout(BROWSER_UPSTREAM_TIMEOUT, self.state.opds.download_book(&acquisition, &self.state.acquisition_directory))
                .await
                .map_err(|_| CatalogServiceError("interactive cover download timed out".to_owned()))?
                .map_err(|error| CatalogServiceError(error.to_string()))?;
            drop(_download_permit);
            let _extraction_permit = tokio::time::timeout(BROWSER_UPSTREAM_TIMEOUT, self.state.extraction_slots.acquire())
                .await
                .map_err(|_| CatalogServiceError("interactive cover extraction capacity timed out".to_owned()))?
                .map_err(|error| CatalogServiceError(error.to_string()))?;
            let path = downloaded.path().to_owned();
            let thumbnail = tokio::task::spawn_blocking(move || generate_publication_thumbnail(&path)).await.map_err(|error| CatalogServiceError(error.to_string()))?.map_err(CatalogServiceError)?;
            drop(downloaded);
            if let Some(thumbnail) = thumbnail {
                return self.state.covers.cover_with(source_id, image_url, async move { Ok(thumbnail) }).await.map_err(|error| CatalogServiceError(error.to_string()));
            }
            if let (true, Some(cache_identity)) = (is_synthetic_cover_key(image_url) || upstream_cover_is_unsuitable, publication_identity.clone()) {
                let cache_source = source_id.clone();
                let cache_url = acquisition.url.clone();
                let cache_image = image_url.to_owned();
                self.cache_write_call(move |cache| cache.clear_active_publication_cover(&cache_source, &cache_identity, &cache_url, &cache_image)).await?;
            }
            return Err(CatalogServiceError("EPUB and catalogue do not contain a suitable cover image".to_owned()));
        }
        if browser_known {
            self.state.covers.cover(source_id, image_url).await.map_err(|error| CatalogServiceError(error.to_string()))
        } else {
            self.state.covers.cached_cover(image_url).await.map_err(|error| CatalogServiceError(error.to_string()))
        }
    }
}
