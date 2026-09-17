//! Execute shared policy in bounded background batches. File/HTTP work never
//! runs inside a database command, and successful ISBN responses serve authors too.
use super::application::{EnrichmentWrite, apply_enrichment_writes};
use super::{OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER, OPEN_LIBRARY_RICH_PROVIDER, enrichment_parallelism};

use crate::{BackendError, library::LibrarySession};
use client_library::prepared_enrichment as prepared;
use metadata_contract::MetadataEnrichmentRequest;
use std::collections::{HashMap, HashSet};

use futures_util::{StreamExt, stream};
use book_enrichment::{SubjectPhase, SubjectStep};

impl LibrarySession {
    pub(super) async fn cached_metadata_enrichment(&self, request: &MetadataEnrichmentRequest) -> Result<metadata_contract::RichMetadataEnrichmentResponse, BackendError> {
        let requested = request.isbns.clone();
        let cached = self.db.cached_isbn_enrichment(requested).map_err(BackendError::operation)?;
        let mut responses = Vec::new();
        let mut missing = Vec::new();
        for (isbn, response) in cached {
            if let Some(response) = response {
                responses.push(response);
            } else {
                missing.push(isbn);
            }
        }
        if !missing.is_empty() {
            let response = self.metadata().enrichment(&MetadataEnrichmentRequest { isbns: missing.clone() }).await.map_err(BackendError::operation)?;
            let mut writes: Vec<EnrichmentWrite> = Vec::new();
            for result in &response.results {
                if !missing.contains(&result.requested_isbn) {
                    continue;
                }
                let places = result.matches.iter().flat_map(|m| m.places.iter().filter_map(|e| e.wikidata_id.as_ref())).collect::<HashSet<_>>();
                let people = result.matches.iter().flat_map(|m| m.people.iter().filter_map(|e| e.wikidata_id.as_ref())).collect::<HashSet<_>>();
                // Store only this ISBN's record, rather than cloning the whole
                // batch once per ISBN. Retain the place graph for parent links.
                let single = metadata_contract::RichMetadataEnrichmentResponse {
                    snapshot: response.snapshot.clone(),
                    results: vec![result.clone()],
                    places: if places.is_empty() { Vec::new() } else { response.places.clone() },
                    people: response.people.iter().filter(|p| people.contains(&p.wikidata_id)).cloned().collect(),
                };
                let isbn = result.requested_isbn.clone();
                let json = serde_json::to_string(&single).map_err(BackendError::operation)?;
                writes.push(Box::new(move |conn| {
                    prepared::cache_isbn_json(conn, &isbn, &json)?;
                    Ok(0)
                }));
            }
            apply_enrichment_writes(self, writes).await?;
            responses.push(response);
        }
        let Some(mut combined) = responses.pop() else { return self.metadata().enrichment(request).await.map_err(BackendError::operation) };
        for response in responses {
            combined.results.extend(response.results);
            combined.places.extend(response.places);
            combined.people.extend(response.people);
        }
        combined.results.sort_by(|a, b| a.requested_isbn.cmp(&b.requested_isbn));
        combined.results.dedup_by(|a, b| a.requested_isbn == b.requested_isbn);
        combined.places.sort_by(|a, b| a.wikidata_id.cmp(&b.wikidata_id));
        combined.places.dedup_by(|a, b| a.wikidata_id == b.wikidata_id);
        combined.people.sort_by(|a, b| a.wikidata_id.cmp(&b.wikidata_id));
        combined.people.dedup_by(|a, b| a.wikidata_id == b.wikidata_id);
        Ok(combined)
    }

    pub(super) async fn enrich_subject_pipeline(&self) -> Result<usize, BackendError> {
        let batch = self.db.upload_preparation_snapshot().map_err(BackendError::operation)?;
        self.enrich_subject_batch(&batch.hashes()).await
    }

    pub(super) async fn enrich_subject_batch(&self, scope: &HashSet<crate::ContentHash>) -> Result<usize, BackendError> {
        let mut updated = 0;
        let scope_json = std::sync::Arc::new(serde_json::to_string(scope).map_err(BackendError::operation)?);
        let through = scope.iter().map(|hash| hash.as_str()).max();
        for phase in SubjectPhase::ALL {
            let mut after = String::new();
            loop {
                if through.is_none_or(|last| !after.is_empty() && after.as_str() >= last) {
                    break;
                }
                let cursor = after.clone();
                let mut jobs = self.db.subject_enrichment_candidates(&cursor, &scope_json).map_err(BackendError::operation)?;
                let Some(last) = jobs.last() else { break };
                after = last.hash.to_string();
                // Finish this phase across every batch before starting any fallback.
                jobs.retain_mut(|job| scope.contains(&job.hash) && job.pipeline.next_step_in_phase(&job.book, phase).is_some());
                if !jobs.is_empty() {
                    let mut isbn_jobs = Vec::new();
                    let mut page_jobs = Vec::new();
                    let mut title_hashes = Vec::new();
                    let mut finished = HashSet::new();
                    let mut exhausted = HashSet::new();
                    let mut failed = HashSet::new();
                    let mut stage_providers = HashMap::new();
                    for job in &mut jobs {
                        match job.pipeline.next_step(&job.book) {
                            SubjectStep::Complete => {
                                finished.insert(job.hash);
                            }
                            SubjectStep::Exhausted => {
                                finished.insert(job.hash);
                                exhausted.insert(job.hash);
                            }
                            SubjectStep::ResolveCodes(_) => {
                                // Book writes already project LCC/BISAC through
                                // the same taxonomy. The shared policy checks whether
                                // the LCC actually resolves before treating it as done.
                                job.pipeline.finish_step();
                            }
                            SubjectStep::LookupIsbns(isbns) => {
                                let exact_isbns = job.book.identifiers.iter().filter(|id| id.scheme() == &book_model::Scheme::Isbn).filter_map(|id| book_enrichment::metadata_isbn(id.value())).collect();
                                let reference_isbns = book_enrichment::related_isbn::lookup_candidates(&job.book.properties).into_iter().collect();
                                isbn_jobs.push(client_library::external_metadata::RichEnrichmentCandidate { content_hash: job.hash, isbns, exact_isbns, reference_isbns, classification_count: job.book.subjects.len() });
                                stage_providers.insert(job.hash, OPEN_LIBRARY_RICH_PROVIDER);
                            }
                            SubjectStep::SearchTitleAuthor => {
                                if job.title.trim().is_empty() {
                                    job.pipeline.finish_step();
                                    continue;
                                }
                                title_hashes.push(job.hash);
                                stage_providers.insert(job.hash, OPEN_LIBRARY_EDITION_IDENTITY_PROVIDER);
                            }
                            step @ (SubjectStep::InspectPages | SubjectStep::InspectOcr) => {
                                let ocr = matches!(step, SubjectStep::InspectOcr);
                                let marker = if ocr { "inspection:ocr-pass" } else { "inspection:page-pass" };
                                let cached = job.book.properties.iter().any(|p| p.property() == marker && p.value() == book_inspection::evidence::INSPECTION_VERSION.to_string());
                                let supported = crate::worker_cpu::inspection_capabilities().supports(job.format, ocr);
                                if cached || !supported {
                                    job.pipeline.finish_step();
                                    continue;
                                }
                                page_jobs.push((job.hash, job.title.clone(), job.format, ocr));
                            }
                        }
                    }
                    let mut pages = stream::iter(page_jobs.into_iter().map(|(hash, title, format, ocr)| async move {
                        let outcome = async {
                            let _lease = self.assets.lease_book(&hash).await.map_err(BackendError::operation)?;
                            let Some(reader) = self.assets.prepare_reader(crate::BlobKind::Book, &hash).await.map_err(BackendError::operation)? else { return Ok(None) };
                            let result = if ocr {
                                crate::worker_cpu::submit_book(crate::worker_cpu::SubjectOcr { source_name: title, format }, reader).await
                            } else {
                                crate::worker_cpu::submit_book(crate::worker_cpu::SubjectPages { source_name: title, format }, reader).await
                            };
                            result.map(Some).map_err(BackendError::operation)
                        }
                        .await;
                        (hash, outcome)
                    }))
                    .buffer_unordered(enrichment_parallelism().0);
                    while let Some((hash, outcome)) = pages.next().await {
                        match outcome {
                            Ok(Some(evidence)) => {
                                // Persist each finished inspection without waiting for a slower sibling.
                                updated += apply_enrichment_writes(self, vec![Box::new(move |conn| prepared::merge_pages(conn, hash, &evidence).map(usize::from))]).await?;
                                jobs.iter_mut().find(|job| job.hash == hash).unwrap().pipeline.finish_step();
                            }
                            Ok(None) => jobs.iter_mut().find(|job| job.hash == hash).unwrap().pipeline.finish_step(),
                            Err(error) => {
                                log::warn!("subject page inspection {hash}: {error}");
                                failed.insert(hash);
                            }
                        }
                    }
                    if !isbn_jobs.is_empty() {
                        let hashes = isbn_jobs.iter().map(|c| c.content_hash).collect::<Vec<_>>();
                        match self.apply_rich_metadata_candidates(isbn_jobs).await {
                            Ok(count) => updated += count,
                            Err(error) => {
                                log::warn!("ISBN stage will retry: {error}");
                                failed.extend(hashes);
                            }
                        }
                    }
                    if !title_hashes.is_empty() {
                        let inputs = self.db.isbn_miss_identity_inputs(title_hashes.clone()).map_err(BackendError::operation)?;
                        match self.resolve_open_library_edition_identities(inputs).await {
                            Ok(count) => updated += count,
                            Err(error) => {
                                log::warn!("title stage will retry: {error}");
                                failed.extend(title_hashes);
                            }
                        }
                    }
                    let hashes = jobs.iter().map(|j| j.hash).collect::<Vec<_>>();
                    let attempted = stage_providers.keys().copied().collect::<HashSet<_>>();
                    let refreshed = self.db.refresh_subject_enrichment(hashes, stage_providers.clone()).map_err(BackendError::operation)?;
                    let mut writes: Vec<EnrichmentWrite> = Vec::new();
                    for refreshed in refreshed {
                        let hash = refreshed.content_hash;
                        let job = jobs.iter_mut().find(|j| j.hash == hash).unwrap();
                        job.book = refreshed.book;
                        // Network failures retain the pending step. An omitted
                        // response is recorded as failed by the existing appliers.
                        if refreshed.provider_succeeded && !failed.contains(&hash) {
                            job.pipeline.finish_step();
                        } else if attempted.contains(&hash) {
                            failed.insert(hash);
                        }
                        let complete = finished.contains(&hash);
                        let delay = if failed.contains(&hash) {
                            60
                        } else if exhausted.contains(&hash) {
                            2_592_000
                        } else {
                            0
                        };
                        let state = job.pipeline.clone();
                        let expected = job.book.clone();
                        let title = job.title.clone();
                        writes.push(Box::new(move |conn| {
                            prepared::save(conn, hash, &state, &expected, &title, complete, delay)?;
                            Ok(0)
                        }));
                    }
                    apply_enrichment_writes(self, writes).await?;
                    crate::executor::sleep(std::time::Duration::from_millis(1)).await;
                }
            }
        }
        Ok(updated)
    }
}
