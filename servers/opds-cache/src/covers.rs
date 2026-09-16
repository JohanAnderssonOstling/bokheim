use super::{
    publication_cover_fallback, publication_metadata_acquisition, CatalogPage, CatalogProcess, CatalogService, CatalogServiceError, CatalogSourceId, BROWSER_UPSTREAM_TIMEOUT, COVER_RECONCILIATION_BATCH, EPUB_EXTRACTION_SWEEP_INTERVAL,
};
use crate::extraction::{generate_publication_thumbnail, is_synthetic_cover_key};
use catalog_model::{CatalogSourcedPublication, CatalogSubjectBrowseEntryPayload};
use catalog_protocol::CatalogPublicationPayload;
use futures_util::future::join_all;
impl CatalogService {
    pub(crate) async fn reconcile_active_cover_downloads(&self) -> Result<(), CatalogServiceError> {
        loop {
            let more = self.store_write_call_named("cover_reconcile", move |cache| cache.reconcile_active_cover_downloads_batch(COVER_RECONCILIATION_BATCH)).await?;
            if !more {
                self.wake_process(CatalogProcess::CoverDownloads);
                return Ok(());
            }
            tokio::task::yield_now().await;
        }
    }

    pub async fn run_cover_download_loop(self) {
        loop {
            if !self.process_enabled(CatalogProcess::CoverDownloads) {
                self.set_process_running(CatalogProcess::CoverDownloads, false);
                self.wait_for_process_wake(CatalogProcess::CoverDownloads).await;
                continue;
            }
            self.set_process_running(CatalogProcess::CoverDownloads, true);
            self.resume_pending_cover_downloads().await;
            self.set_process_running(CatalogProcess::CoverDownloads, false);
            tokio::select! {
                _ = tokio::time::sleep(EPUB_EXTRACTION_SWEEP_INTERVAL) => {}
                _ = self.wait_for_process_wake(CatalogProcess::CoverDownloads) => {}
            }
        }
    }

    pub(crate) async fn resume_pending_cover_downloads(&self) {
        if let Err(error) = self.reconcile_active_cover_downloads().await {
            tracing::warn!(%error, "could not reconcile the durable cover download queue");
        }
        while self.process_enabled(CatalogProcess::CoverDownloads) {
            let job = match self.store_write_call_named("cover_claim", |cache| cache.claim_next_cover_download()).await {
                Ok(Some(job)) => job,
                Ok(None) => break,
                Err(error) => {
                    tracing::warn!(%error, "could not claim a cover download");
                    break;
                }
            };
            self.download_queued_cover(job).await;
        }
    }

    async fn download_queued_cover(&self, job: super::CatalogCoverDownloadJob) {
        self.state.covers.remember_cover_url(&job.source_id, &job.image_url);
        let outcome_error = self.state.covers.cover(&job.source_id, &job.image_url).await.err().map(|error| error.to_string());
        let cache_job = job.clone();
        if let Err(error) = self.store_write_call_named("cover_finish", move |cache| cache.finish_cover_download(&cache_job, outcome_error.as_deref().map_or(Ok(()), Err))).await {
            tracing::warn!(source = job.source_id.as_str(), edition = job.publication.edition_id, %error, "could not persist cover download outcome");
        }
    }

    pub(crate) async fn prepare_publication_cover_fallbacks(&self, source_id: &CatalogSourceId, page: &mut CatalogPage) {
        for publication in page.publications.iter_mut().chain(page.groups.iter_mut().flat_map(|group| group.publications.iter_mut())) {
            let _ = publication_cover_fallback(source_id, publication);
        }
    }

    pub(crate) async fn publicize_payload_covers(&self, payloads: &mut [CatalogPublicationPayload]) {
        let image_urls = payloads.iter().map(|payload| payload.publication.image_url.clone()).collect::<Vec<_>>();
        let references = self.public_cover_references(&image_urls).await;
        for (payload, reference) in payloads.iter_mut().zip(references) {
            if let Some(reference) = reference {
                payload.publication.image_url = Some(reference);
            }
        }
    }

    pub(crate) async fn publicize_subject_covers(&self, payloads: &mut [CatalogSubjectBrowseEntryPayload]) {
        let image_urls = payloads.iter().map(|payload| payload.publication.publication.image_url.clone()).collect::<Vec<_>>();
        let references = self.public_cover_references(&image_urls).await;
        for (payload, reference) in payloads.iter_mut().zip(references) {
            if let Some(reference) = reference {
                payload.publication.publication.image_url = Some(reference);
            }
        }
    }

    pub(crate) async fn publicize_sourced_covers(&self, publications: &mut [CatalogSourcedPublication]) {
        let image_urls = publications.iter().map(|publication| publication.publication.image_url.clone()).collect::<Vec<_>>();
        let references = self.public_cover_references(&image_urls).await;
        for (publication, reference) in publications.iter_mut().zip(references) {
            if let Some(reference) = reference {
                publication.publication.image_url = Some(reference);
            }
        }
    }

    async fn public_cover_references(&self, image_urls: &[Option<String>]) -> Vec<Option<String>> {
        join_all(image_urls.iter().map(|image_url| async {
            match image_url {
                Some(image_url) => self.state.covers.public_cover_reference(image_url).await,
                None => None,
            }
        }))
        .await
    }

    pub async fn catalog_cover_asset(&self, digest: &str) -> Result<Vec<u8>, CatalogServiceError> {
        self.state.covers.cached_cover_asset(digest).await.map_err(|error| CatalogServiceError(error.to_string()))
    }

    pub async fn catalog_cover(&self, source_id: &CatalogSourceId, image_url: &str) -> Result<Vec<u8>, CatalogServiceError> {
        if let Some(digest) = catalog_core::catalog_cover_asset_digest(image_url) {
            return self.catalog_cover_asset(digest).await;
        }
        self.public_endpoint(source_id).await?;
        let cache_source = source_id.clone();
        let cache_image = image_url.to_owned();
        let active = self.store_call(move |store| store.active_publication_for_cover(&cache_source, &cache_image)).await?.ok_or_else(|| CatalogServiceError("cover URL is not present in the active catalog generation".to_owned()))?;
        self.state.covers.remember_cover_url(source_id, image_url);
        let publication_identity = active.identity.clone();
        let epub_acquisition = publication_metadata_acquisition(&active);
        if let Some(acquisition) = epub_acquisition {
            let mut upstream_cover_is_unsuitable = false;
            // A real OPDS URL may already have produced a sufficiently large,
            // cover-shaped cached thumbnail. Synthetic keys always represent
            // an EPUB fallback and therefore skip this probe.
            if !is_synthetic_cover_key(image_url) {
                match self.state.covers.cached_cover(image_url).await {
                    Ok(bytes) => return Ok(bytes),
                    Err(error) if error.is_unsuitable_cover_dimensions() => upstream_cover_is_unsuitable = true,
                    Err(_error) if !is_synthetic_cover_key(image_url) => match self.state.covers.cover(source_id, image_url).await {
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
            if self.process_enabled(CatalogProcess::Enrichment) {
                let cache_source = source_id.clone();
                let cache_url = acquisition.url.clone();
                self.store_write_call(move |cache| cache.queue_epub_extractions(&cache_source, std::slice::from_ref(&cache_url), false)).await?;
                if let Err(error) = self.enrich_active_publication(source_id, publication_identity.clone(), acquisition.clone(), Some(image_url.to_owned())).await {
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
                    let status = self.store_call(move |cache| cache.epub_extraction_status(&cache_source, &cache_url)).await?;
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
            if is_synthetic_cover_key(image_url) || upstream_cover_is_unsuitable {
                let cache_source = source_id.clone();
                let cache_url = acquisition.url.clone();
                let cache_image = image_url.to_owned();
                self.store_write_call(move |store| store.clear_active_publication_cover(&cache_source, &publication_identity, &cache_url, &cache_image)).await?;
            }
            return Err(CatalogServiceError("EPUB and catalogue do not contain a suitable cover image".to_owned()));
        }
        self.state.covers.cached_cover(image_url).await.map_err(|error| CatalogServiceError(error.to_string()))
    }
}
