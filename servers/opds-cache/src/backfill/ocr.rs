use super::super::{
    extraction_book_format, prepare_catalog_subject_assignments, book_has_acquisition_url, CatalogOcrBackfillJob, CatalogProcess, CatalogService, CatalogServiceError, OpdsAcquisitionKind, OpdsDirectAcquisition,
    OCR_BACKFILL_BATCH_SIZE, OCR_BACKFILL_MAX_JOBS_PER_PASS,
};
use crate::extraction::{extract_catalog_ocr_isolated, merge_catalog_book_subjects};

impl CatalogService {
    /// Runs after the metadata/title-author wave has drained. OCR is kept at
    /// one job at a time because it is CPU- and memory-heavy, while its claim
    /// remains durable across restarts and failures.
    pub(crate) async fn resume_ocr_backfill(&self) {
        if !self.process_enabled(CatalogProcess::Enrichment) {
            return;
        }
        let mut processed = 0;
        loop {
            if !self.process_enabled(CatalogProcess::Enrichment) {
                return;
            }
            let jobs = match self.store_write_call_named("ocr_backfill_claim", |store| store.claim_ocr_backfill_batch(OCR_BACKFILL_BATCH_SIZE)).await {
                Ok(jobs) if jobs.is_empty() => return,
                Ok(jobs) => jobs,
                Err(error) => {
                    tracing::warn!(%error, "could not claim catalogue OCR backfill batch");
                    return;
                }
            };
            for job in jobs {
                processed += 1;
                let retry_job = job.clone();
                if let Err(error) = self.run_ocr_backfill_job(job).await {
                    tracing::warn!(source = retry_job.source_id.as_str(), url = retry_job.acquisition_url, %error, "catalogue OCR backfill failed; retaining the durable retry");
                    let message = error.to_string();
                    let _ = self.store_write_call(move |store| store.release_ocr_backfill(&[retry_job], &message)).await;
                    // Do not sleep inside the catalogue pass. The retry is
                    // durable; description and index maintenance must remain
                    // runnable while this job waits for a later pass.
                    return;
                }
                if processed >= OCR_BACKFILL_MAX_JOBS_PER_PASS {
                    return;
                }
            }
        }
    }

    async fn run_ocr_backfill_job(&self, job: CatalogOcrBackfillJob) -> Result<(), CatalogServiceError> {
        let source_id = job.source_id.clone();
        let identity = job.book.clone();
        let acquisition_url = job.acquisition_url.clone();
        let Some(book) = self
            .store_call({
                let source_id = source_id.clone();
                let identity = identity.clone();
                move |store| store.active_book(&source_id, &identity)
            })
            .await?
        else {
            let attempt = job.attempt;
            self.store_write_call(move |store| store.mark_ocr_not_needed(&source_id, &acquisition_url, attempt)).await?;
            return Ok(());
        };

        let already_resolved = book
            .metadata
            .subjects
            .iter()
            .any(|subject| subject.authority.as_deref().is_some_and(|authority| authority.eq_ignore_ascii_case("lcc")) && subject.code.as_deref().is_some_and(|code| !subject_projection::unified_lcc_subject_paths(code).is_empty()));
        if already_resolved {
            let attempt = job.attempt;
            self.store_write_call(move |store| store.mark_ocr_not_needed(&source_id, &acquisition_url, attempt)).await?;
            return Ok(());
        }

        let book_format = extraction_book_format(job.book_format).ok_or_else(|| CatalogServiceError("catalogue OCR job has an unsupported book format".to_owned()))?;
        let digest = blake3::hash(job.acquisition_url.as_bytes()).to_hex();
        let acquisition = OpdsDirectAcquisition { url: job.acquisition_url.clone(), kind: OpdsAcquisitionKind::OpenAccess, book_format, suggested_file_name: format!("{digest}.{}", book_format.extension()), price: None };
        let downloaded = self.download_catalog_metadata(&acquisition, false, false).await?;
        let extraction_permit = self.state.extraction_slots.acquire().await.map_err(|error| CatalogServiceError(error.to_string()))?;
        let extracted = extract_catalog_ocr_isolated(downloaded.path(), book_format).await?;
        drop(extraction_permit);
        drop(downloaded);

        // OCR runs without the source lock, but the final merge is based on a
        // fresh book snapshot so a crawl or another enrichment cannot
        // be overwritten by the stale pre-OCR payload.
        let update_lock = self.source_update_lock(&source_id).await;
        let _update = update_lock.lock().await;
        let Some(mut book) = self
            .store_call({
                let source_id = source_id.clone();
                let identity = identity.clone();
                move |store| store.active_book(&source_id, &identity)
            })
            .await?
        else {
            let attempt = job.attempt;
            self.store_write_call(move |store| store.mark_ocr_not_needed(&source_id, &acquisition_url, attempt)).await?;
            return Ok(());
        };
        if !book_has_acquisition_url(&book, &acquisition_url) {
            let attempt = job.attempt;
            self.store_write_call(move |store| store.mark_ocr_not_needed(&source_id, &acquisition_url, attempt)).await?;
            return Ok(());
        }
        merge_catalog_book_subjects(&mut book, &extracted.subjects);
        let subjects = book
            .metadata
            .subjects
            .iter()
            .map(|subject| book_model::BookSubject::new(None, &subject.name, &subject.source, subject.authority.clone(), subject.code.clone()).map_err(|error| CatalogServiceError(error.to_string())))
            .collect::<Result<Vec<_>, _>>()?;
        let assignments = prepare_catalog_subject_assignments(&subjects);
        let source_for_write = job.source_id.clone();
        let url_for_write = job.acquisition_url.clone();
        self.store_write_call_named("ocr_backfill_finish", move |store| store.finish_ocr_backfill(&source_for_write, &url_for_write, &book, &subjects, &assignments, job.attempt)).await?;
        Ok(())
    }
}
