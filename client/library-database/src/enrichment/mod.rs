//! Enrichment work queues, results, and thumbnail jobs.


// ---- enrichment ----
// Typed thumbnail-work queue operations.


use crate::{Database, DatabaseError};
use sync_common::ContentHash;
impl Database {
    pub fn pending_thumbnail_job_count(&self) -> Result<u64, DatabaseError> {
        let connection = &self.connection;
        Ok(connection.thumbnail_pending_job_count(|row| row.get::<_, i64>(0))?.max(0) as u64)
    }

    pub fn pending_thumbnail_jobs_page(&self, after: &str) -> Result<Vec<(ContentHash, i64)>, DatabaseError> {
        let connection = &self.connection;
        let mut jobs = Vec::new();
        connection.thumbnail_pending_jobs(after, |row| {
            jobs.push((ContentHash::new(&row.get::<_, String>(0)?), row.get(1)?));
            Ok(())
        })?;
        Ok(jobs)
    }

    pub fn thumbnail_book_format(&self, hash: ContentHash) -> Result<Option<book_model::BookFormat>, DatabaseError> {
        let connection = &self.connection;
        let mut format = None;
        connection.thumbnail_local_format(hash.as_str(), |row| {
            format = Some(row.get::<_, String>(0)?);
            Ok(())
        })?;
        format.map(|format| book_model::BookFormat::from_extension(&format)
            .ok_or_else(|| DatabaseError::operation("book format is missing")))
            .transpose()
    }

    pub fn confirm_thumbnail_available(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        let connection = &self.connection;
        connection.thumbnail_set_result(hash.as_str(), "ready")?;
        Ok(())
    }

    pub fn has_remote_thumbnail(&self, hash: &ContentHash) -> Result<bool, DatabaseError> {
        let connection = &self.connection;
        connection.thumbnail_has_remote(hash.as_str(), |row| row.get(0)).map_err(Into::into)
    }

    pub fn defer_thumbnail_to_download(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        let connection = &self.connection;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.thumbnail_discard(hash.as_str())?;
        transaction.thumbnail_enqueue_asset_work(hash.as_str())?;
        transaction.commit()?;
        Ok(())
    }

    pub fn discard_thumbnail_work(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        let connection = &self.connection;
        connection.thumbnail_discard(hash.as_str())?;
        Ok(())
    }

    pub fn has_book_placement(&self, hash: ContentHash) -> Result<bool, DatabaseError> {
        Ok(self.thumbnail_book_format(hash)?.is_some())
    }

    pub fn complete_thumbnail_work(&self, hash: &ContentHash, has_cover: bool) -> Result<(), DatabaseError> {
        let connection = &self.connection;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.thumbnail_confirm_source(hash.as_str(), "local")?;
        transaction.thumbnail_set_result(hash.as_str(), if has_cover { "ready" } else { "no_cover" })?;
        transaction.commit()?;
        Ok(())
    }

    pub fn retry_thumbnail_work(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        let connection = &self.connection;
        let retry_after = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .map_err(DatabaseError::operation)?.as_secs().saturating_add(30) as i64;
        connection.thumbnail_retry(hash.as_str(), retry_after)?;
        Ok(())
    }

    pub fn thumbnail_upload_allowed(&self, hash: &ContentHash) -> Result<bool, DatabaseError> {
        let connection = &self.connection;
        let mut allowed = false;
        connection.thumbnail_upload_allowed(hash.as_str(), |row| {
            allowed = row.get(0)?;
            Ok(())
        })?;
        Ok(allowed)
    }

    pub fn record_uploaded_thumbnail(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        let connection = &self.connection;
        connection.thumbnail_record_remote(hash.as_str())?;
        Ok(())
    }

    pub fn complete_downloaded_thumbnail(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        let connection = &self.connection;
        let transaction = self.connection.unchecked_transaction()?;
        transaction.thumbnail_record_remote(hash.as_str())?;
        transaction.thumbnail_confirm_source(hash.as_str(), "remote")?;
        transaction.thumbnail_set_result(hash.as_str(), "ready")?;
        transaction.commit()?;
        Ok(())
    }

    pub fn forget_remote_thumbnail(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        let connection = &self.connection;
        connection.thumbnail_forget_remote(hash.as_str())?;
        Ok(())
    }

    pub fn request_missing_thumbnail(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        let connection = &self.connection;
        connection.thumbnail_request_missing(hash.as_str())?;
        Ok(())
    }

    /// Queues thumbnail recovery unconditionally, resetting any backoff.
    /// Unlike [`Database::request_missing_thumbnail`], which only revives
    /// finished work, this also covers books that never had a work row.
    pub fn request_thumbnail(&self, hash: &ContentHash) -> Result<(), DatabaseError> {
        self.connection.request_thumbnail(hash.as_str())?;
        Ok(())
    }

    /// Current thumbnail work state and retry deadline, if any row exists.
    pub fn thumbnail_work_state(&self, hash: &ContentHash) -> Result<Option<(String, i64)>, DatabaseError> {
        let mut state = None;
        self.connection.thumbnail_work_state(hash.as_str(), |row| {
            state = Some((row.get::<_, String>(0)?, row.get::<_, i64>(1)?));
            Ok(())
        })?;
        Ok(state)
    }

    /// Records remote thumbnail availability and returns missing hashes still
    /// eligible for local generation/upload.
    pub fn settle_thumbnail_availability(
        &self,
        available: &[ContentHash],
        missing: &[ContentHash],
    ) -> Result<Vec<ContentHash>, DatabaseError> {
        let connection = &self.connection;
        let transaction = self.connection.unchecked_transaction()?;
        for hash in available {
            transaction.thumbnail_record_remote(hash.as_str())?;
        }
        let mut uploads = Vec::new();
        for hash in missing {
            use rusqlite::OptionalExtension as _;
            let mut allowed = false;
            transaction.thumbnail_upload_allowed(hash.as_str(), |row| {
                allowed = row.get(0)?;
                Ok(())
            }).optional()?;
            if allowed {
                uploads.push(*hash);
            }
        }
        transaction.commit().map_err(DatabaseError::operation)?;
        Ok(uploads)
    }
}

include_sql!("src/enrichment/enrichment.sql");

// ---- work ----
// Upload preparation snapshots and Audible batch claims.

#[derive(Clone, Debug)]
pub struct UploadPreparationBatch {
    pub members: Vec<(ContentHash, Option<i64>)>,
}

impl UploadPreparationBatch {
    pub fn hashes(&self) -> std::collections::HashSet<ContentHash> {
        self.members.iter().map(|(hash, _)| *hash).collect()
    }
}

#[derive(Clone, Debug)]
pub struct AudibleBatchJob {
    pub content_hash: ContentHash,
    pub request: metadata_contract::audible::LookupRequest,
    pub token: String,
}

impl Database {
    pub fn upload_preparation_snapshot(&self) -> Result<UploadPreparationBatch, DatabaseError> {
        let mut members = Vec::new();
        self.connection.upload_preparation_snapshot(|row| {
            members.push((ContentHash::new(&row.get::<_, String>(0)?), row.get::<_, Option<i64>>(1)?));
            Ok(())
        })?;
        Ok(UploadPreparationBatch { members })
    }

    pub fn prepared_upload_batch(&self, scope: &std::collections::HashSet<ContentHash>) -> Result<UploadPreparationBatch, DatabaseError> {
        let scope_json = serde_json::to_string(scope).map_err(DatabaseError::operation)?;
        let mut members = Vec::new();
        self.connection.prepared_upload_batch(scope_json.as_str(), |row| {
            members.push((ContentHash::new(&row.get::<_, String>(0)?), row.get::<_, Option<i64>>(1)?));
            Ok(())
        })?;
        Ok(UploadPreparationBatch { members })
    }

    pub fn finish_upload_preparation(&self, batch: &UploadPreparationBatch) -> Result<(), DatabaseError> {
        let members: Vec<(String, Option<i64>)> = batch.members.iter().map(|(hash, id)| (hash.to_string(), *id)).collect();
        let json = serde_json::to_string(&members).map_err(DatabaseError::operation)?;
        self.connection.finish_upload_preparation(json.as_str(), metadata_contract::audible::POLICY_VERSION)?;
        Ok(())
    }

    pub fn upload_thumbnail_candidates(&self, members: &std::collections::HashSet<ContentHash>, now: i64) -> Result<Vec<ContentHash>, DatabaseError> {
        let scope_json = serde_json::to_string(members).map_err(DatabaseError::operation)?;
        let mut hashes = Vec::new();
        self.connection.upload_thumbnail_candidates(scope_json.as_str(), now, |row| {
            hashes.push(ContentHash::new(&row.get::<_, String>(0)?));
            Ok(())
        })?;
        Ok(hashes)
    }

    pub fn audible_batch_candidates(&self, now: i64, members: &[ContentHash]) -> Result<Vec<ContentHash>, DatabaseError> {
        let json = serde_json::to_string(members).map_err(DatabaseError::operation)?;
        let mut hashes = Vec::new();
        self.connection.audible_batch_candidates(json.as_str(), now, metadata_contract::audible::POLICY_VERSION, |row| {
            hashes.push(ContentHash::new(&row.get::<_, String>(0)?));
            Ok(())
        })?;
        Ok(hashes)
    }

    pub fn claim_audible_batch(&self, now: i64, hash: ContentHash) -> Result<Option<AudibleBatchJob>, DatabaseError> {
        let mut request_json = None;
        self.connection.audible_claim_candidate(hash.as_str(), now, metadata_contract::audible::POLICY_VERSION, |row| {
            request_json = Some(row.get::<_, String>(0)?);
            Ok(())
        })?;
        let Some(request_json) = request_json else { return Ok(None) };
        let request: metadata_contract::audible::LookupRequest = serde_json::from_str(&request_json).map_err(DatabaseError::operation)?;
        let token = uuid::Uuid::new_v4().to_string();
        self.connection.audible_claim(hash.as_str(), token.as_str(), now + 300, now)?;
        Ok(Some(AudibleBatchJob { content_hash: hash, request, token }))
    }

    pub fn resolve_audible_batch(&self, job: &AudibleBatchJob, response: &metadata_contract::audible::LookupResponse, retry_at: i64, now: i64) -> Result<bool, DatabaseError> {
        use metadata_contract::audible::LookupStatus;
        let status = match response.status {
            LookupStatus::Supported => "applied",
            LookupStatus::NoMatch => "no_match",
            LookupStatus::Ambiguous => "ambiguous",
            LookupStatus::Unavailable => "unavailable",
        };
        let response_json = serde_json::to_string(response).map_err(DatabaseError::operation)?;
        let committed = self.connection.audible_resolve(job.content_hash.as_str(), job.token.as_str(), status, response_json.as_str(), retry_at, now)?;
        if committed == 0 {
            return Ok(false);
        }
        if status == "applied" {
            if let Some(plan) = response.chapter_plan.as_ref() {
                let toc_json = serde_json::to_string(plan).map_err(DatabaseError::operation)?;
                self.connection.audible_store_toc(job.content_hash.as_str(), plan.chapters.len() as i64, toc_json.as_str())?;
            }
        }
        Ok(true)
    }
}

// ---- results ----
// Enrichment candidates, result application, and status reads.

#[derive(Clone, Debug)]
pub struct AuthorCredit {
    pub position: i64,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct AuthorEnrichmentCandidate {
    pub content_hash: ContentHash,
    pub isbns: Vec<String>,
    pub authors: Vec<AuthorCredit>,
}

#[derive(Clone, Debug)]
pub struct EditionIdentityCandidate {
    pub content_hash: ContentHash,
    pub title: String,
    pub authors: Vec<String>,
    pub publishers: Vec<String>,
    pub book_year: Option<i32>,
    pub resource_isbns: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct DescriptionEnrichmentResult {
    pub content_hash: ContentHash,
    pub provider: String,
    pub identifier: String,
    pub description: Option<String>,
    pub status: String,
}

#[derive(Clone, Debug)]
pub struct EnrichmentFailure {
    pub content_hash: ContentHash,
    pub provider: String,
    pub identifier: String,
    pub detail: String,
}

#[derive(Clone, Debug)]
pub struct EnrichmentStatusCounts {
    pub total: i64,
    pub enriched: i64,
    pub pending: i64,
    pub no_match: i64,
    pub failed: i64,
}

#[derive(Clone, Debug)]
pub struct SubjectEnrichmentJob {
    pub hash: ContentHash,
    pub title: String,
    pub format: book_model::BookFormat,
    pub book: book_model::BookMetadata,
    pub pipeline: book_enrichment::SubjectPipeline,
}

#[derive(Clone, Debug)]
pub struct RefreshedSubject {
    pub content_hash: ContentHash,
    pub book: book_model::BookMetadata,
    pub provider_succeeded: bool,
}

fn split_isbns(value: &str) -> Vec<String> {
    value.split(',').map(str::trim).filter(|part| !part.is_empty()).map(str::to_owned).collect()
}

impl Database {
    pub fn description_enrichment_candidates(&self, provider: &str, cursor: &str, scope: &str) -> Result<Vec<(ContentHash, Vec<String>)>, DatabaseError> {
        let mut candidates = Vec::new();
        self.connection.enrichment_description_candidates(provider, cursor, scope, |row| {
            candidates.push((ContentHash::new(&row.get::<_, String>(0)?), split_isbns(&row.get::<_, String>(1)?)));
            Ok(())
        })?;
        Ok(candidates)
    }

    pub fn author_enrichment_candidates(&self, provider: &str, cursor: &str, scope: &str) -> Result<Vec<AuthorEnrichmentCandidate>, DatabaseError> {
        let mut candidates = Vec::new();
        self.connection.enrichment_author_candidates(provider, cursor, scope, |row| {
            let content_hash = ContentHash::new(&row.get::<_, String>(0)?);
            let isbns = split_isbns(&row.get::<_, String>(1)?);
            let mut authors = Vec::new();
            self.connection.enrichment_author_credits(content_hash.as_str(), |credit| {
                authors.push(AuthorCredit { position: credit.get(0)?, name: credit.get(1)? });
                Ok(())
            })?;
            candidates.push(AuthorEnrichmentCandidate { content_hash, isbns, authors });
            Ok(())
        })?;
        Ok(candidates)
    }

    pub fn cover_enrichment_candidates(&self, provider: &str, cursor: &str, scope: &str) -> Result<Vec<(ContentHash, Vec<String>)>, DatabaseError> {
        let mut candidates = Vec::new();
        self.connection.enrichment_cover_candidates(provider, cursor, scope, |row| {
            candidates.push((ContentHash::new(&row.get::<_, String>(0)?), split_isbns(&row.get::<_, String>(1)?)));
            Ok(())
        })?;
        Ok(candidates)
    }

    pub fn isbn_miss_identity_inputs(&self, title_hashes: Vec<ContentHash>) -> Result<Vec<EditionIdentityCandidate>, DatabaseError> {
        let mut candidates = Vec::new();
        for hash in title_hashes {
            let mut input = None;
            self.connection.enrichment_identity_input(hash.as_str(), |row| {
                input = Some((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?));
                Ok(())
            })?;
            let Some((title, metadata)) = input else { continue };
            let metadata = crate::sync::decode_book_metadata(&metadata)?;
            let mut authors = Vec::new();
            self.connection.enrichment_identity_authors(hash.as_str(), |row| {
                authors.push(row.get::<_, String>(0)?);
                Ok(())
            })?;
            let publishers = metadata.publishers.iter().map(|credit| credit.name().as_str().to_owned()).collect();
            let book_year = metadata.dates.iter().filter_map(|date| date.value().get(..4).and_then(|year| year.parse::<i32>().ok())).next();
            let resource_isbns = metadata
                .identifiers
                .iter()
                .filter(|identifier| identifier.scheme() == &book_model::Scheme::Isbn)
                .filter_map(|identifier| identifier.canonical_value())
                .collect();
            candidates.push(EditionIdentityCandidate { content_hash: hash, title, authors, publishers, book_year, resource_isbns });
        }
        Ok(candidates)
    }

    pub fn subject_enrichment_candidates(&self, cursor: &str, scope: &str) -> Result<Vec<SubjectEnrichmentJob>, DatabaseError> {
        let mut jobs = Vec::new();
        self.connection.enrichment_subject_candidates(cursor, scope, |row| {
            let hash = ContentHash::new(&row.get::<_, String>(0)?);
            let title: String = row.get(1)?;
            let Some(format) = row.get::<_, String>(2).ok().and_then(|format| book_model::BookFormat::from_extension(&format).ok()) else {
                return Ok(());
            };
            let Ok(book) = crate::sync::decode_book_metadata(&row.get::<_, Vec<u8>>(3)?) else {
                return Ok(());
            };
            let pipeline = row
                .get::<_, Option<String>>(4)
                .ok()
                .flatten()
                .and_then(|state| serde_json::from_str(&state).ok())
                .unwrap_or_default();
            jobs.push(SubjectEnrichmentJob { hash, title, format, book, pipeline });
            Ok(())
        })?;
        Ok(jobs)
    }

    pub fn refresh_subject_enrichment(&self, hashes: Vec<ContentHash>, providers: std::collections::HashMap<ContentHash, &str>) -> Result<Vec<RefreshedSubject>, DatabaseError> {
        let mut refreshed = Vec::new();
        for hash in hashes {
            let mut raw = None;
            self.connection.enrichment_book(hash.as_str(), |row| {
                raw = Some(row.get::<_, Vec<u8>>(0)?);
                Ok(())
            })?;
            let Some(raw) = raw else { continue };
            let book = crate::sync::decode_book_metadata(&raw)?;
            let provider_succeeded = match providers.get(&hash) {
                Some(provider) => self
                    .connection
                    .enrichment_step_succeeded(hash.as_str(), provider, |row| row.get::<_, bool>(0))
                    .unwrap_or(false),
                None => false,
            };
            refreshed.push(RefreshedSubject { content_hash: hash, book, provider_succeeded });
        }
        Ok(refreshed)
    }

    pub fn cached_isbn_enrichment(&self, isbns: Vec<String>) -> Result<Vec<(String, Option<String>)>, DatabaseError> {
        let mut cached = Vec::new();
        for isbn in isbns {
            let mut response = None;
            self.connection.enrichment_cached_isbn(isbn.as_str(), |row| {
                response = Some(row.get::<_, String>(0)?);
                Ok(())
            })?;
            cached.push((isbn, response));
        }
        Ok(cached)
    }

    pub fn apply_description_enrichment(&self, result: DescriptionEnrichmentResult) -> Result<usize, DatabaseError> {
        let transaction = self.connection.unchecked_transaction()?;
        let updated = transaction.enrichment_set_missing_description(result.content_hash.as_str(), result.description.as_deref().unwrap_or(""))?;
        transaction.enrichment_mark_description_scanned(result.content_hash.as_str())?;
        transaction.enrichment_record_attempt(
            result.content_hash.as_str(),
            result.provider.as_str(),
            result.identifier.as_str(),
            result.status.as_str(),
            result.description.as_deref(),
        )?;
        transaction.commit()?;
        Ok(updated)
    }

    pub fn record_enrichment_failures(&self, failures: Vec<EnrichmentFailure>) -> Result<(), DatabaseError> {
        for failure in failures {
            self.connection.enrichment_record_attempt(
                failure.content_hash.as_str(),
                failure.provider.as_str(),
                failure.identifier.as_str(),
                "failed",
                Some(failure.detail.as_str()),
            )?;
        }
        Ok(())
    }

    pub fn cover_enrichment_state(&self, hash: &ContentHash) -> Result<(Option<(String, i64)>, Option<book_model::BookFormat>), DatabaseError> {
        let mut state = None;
        self.connection.enrichment_thumbnail_state(hash.as_str(), |row| {
            state = Some((row.get::<_, String>(0)?, row.get::<_, i64>(1)?));
            Ok(())
        })?;
        let mut format = None;
        self.connection.enrichment_book_format(hash.as_str(), |row| {
            format = row.get::<_, String>(0)?;
            Ok(())
        })?;
        Ok((state, format.and_then(|format| book_model::BookFormat::from_extension(&format).ok())))
    }

    pub fn enrichment_status_counts(&self, providers: [&str; 4]) -> Result<EnrichmentStatusCounts, DatabaseError> {
        self.connection.enrichment_status_counts(providers[0], providers[1], providers[2], providers[3], |row| {
            Ok(EnrichmentStatusCounts {
                total: row.get(0)?,
                enriched: row.get(1)?,
                pending: row.get(2)?,
                no_match: row.get(3)?,
                failed: row.get(4)?,
            })
        })
    }
}
