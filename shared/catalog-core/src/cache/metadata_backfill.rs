use super::{
    active_generation_in, advance_catalog_transport_revision, bounded_crawl_error, decode_cached, merge_catalog_subject_evidence, merge_extracted_author_names, merge_extracted_publication_authors, metadata_index_from_metadata, params,
    prepare_catalog_subject_assignments, record_identity_review, record_identity_review_failure, sequence_in, sequenced_publication_in, sql_decode_error, storage_i64, unix_timestamp, upsert_active_publication, CatalogCache,
    CatalogCacheError, CatalogEpubMetadata, CatalogMetadataBackfillIndexedJob, CatalogMetadataBackfillIndexedUpdate, CatalogMetadataBackfillJob, CatalogMetadataBackfillUpdate, CatalogMetadataBackfillWriteTimings, CatalogMetadataIndex,
    CatalogPublicationIdentity, CatalogSourceId, CatalogSubjectAssignment, Instant, TransactionBehavior, CATALOG_METADATA_BACKFILL_VERSION, CATALOG_METADATA_INDEX_VERSION,
};

impl CatalogCache {
    pub fn schedule_metadata_backfill(&self) -> Result<usize, CatalogCacheError> {
        let connection = self.pool.get()?;
        connection
            .execute(
                "INSERT INTO catalog_metadata_backfill(source_id,acquisition_url,version,status)
                 SELECT extraction.source_id,extraction.acquisition_url,?1,'pending'
                 FROM catalog_epub_extraction extraction
                 JOIN catalog_source source ON source.source_id=extraction.source_id
                 JOIN catalog_publication publication
                   ON publication.source_id=extraction.source_id AND publication.generation=source.active_generation
                  AND publication.acquisition_url=extraction.acquisition_url
                 WHERE extraction.status='succeeded' AND extraction.metadata_payload IS NOT NULL
                   AND NOT EXISTS(
                       SELECT 1 FROM catalog_metadata_backfill current
                       WHERE current.source_id=extraction.source_id AND current.acquisition_url=extraction.acquisition_url AND current.version=?1
                   )
                 LIMIT 1000
                 ON CONFLICT(source_id,acquisition_url) DO UPDATE SET
                    version=excluded.version,status='pending',last_error=NULL
                 WHERE catalog_metadata_backfill.version!=excluded.version",
                [CATALOG_METADATA_BACKFILL_VERSION],
            )
            .map_err(Into::into)
    }
    pub fn migrate_metadata_index_batch(&self, limit: usize) -> Result<usize, CatalogCacheError> {
        let limit = limit.clamp(1, 256);
        let mut connection = self.pool.get()?;
        let rows = {
            let mut statement = connection.prepare(
                "SELECT extraction.source_id,extraction.acquisition_url,extraction.metadata_payload
                 FROM catalog_epub_extraction extraction
                 LEFT JOIN catalog_metadata_index metadata_index
                   ON metadata_index.source_id=extraction.source_id AND metadata_index.acquisition_url=extraction.acquisition_url
                 WHERE extraction.status='succeeded' AND extraction.metadata_payload IS NOT NULL
                   AND (metadata_index.version IS NULL OR metadata_index.version!=?1)
                 ORDER BY extraction.source_id,extraction.acquisition_url LIMIT ?2",
            )?;
            let rows = statement.query_map(params![CATALOG_METADATA_INDEX_VERSION, storage_i64(limit as u64, "metadata index batch size")?], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Vec<u8>>(2)?)))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let prepared = rows
            .iter()
            .map(|(source_id, acquisition_url, payload)| {
                let metadata: CatalogEpubMetadata = decode_cached(payload).map_err(sql_decode_error)?;
                let index = metadata_index_from_metadata(&metadata);
                Ok::<_, CatalogCacheError>((source_id.clone(), acquisition_url.clone(), index.title, index.publication_year, wire::encode(&index.authors)?, wire::encode(&index.publishers)?, wire::encode(&index.isbns)?))
            })
            .collect::<Result<Vec<_>, CatalogCacheError>>()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (source_id, acquisition_url, title, publication_year, authors_payload, publishers_payload, isbns_payload) in &prepared {
            transaction.execute(
                "INSERT INTO catalog_metadata_index(source_id,acquisition_url,version,title,publication_year,authors_payload,publishers_payload,isbns_payload)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
                 ON CONFLICT(source_id,acquisition_url) DO UPDATE SET
                   version=excluded.version,title=excluded.title,publication_year=excluded.publication_year,
                   authors_payload=excluded.authors_payload,publishers_payload=excluded.publishers_payload,isbns_payload=excluded.isbns_payload",
                params![source_id, acquisition_url, CATALOG_METADATA_INDEX_VERSION, title, publication_year, authors_payload, publishers_payload, isbns_payload],
            )?;
        }
        transaction.commit()?;
        Ok(prepared.len())
    }

    pub fn claim_metadata_backfill_indexed_batch(&self, limit: usize) -> Result<Vec<CatalogMetadataBackfillIndexedJob>, CatalogCacheError> {
        let limit = limit.clamp(1, 256);
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let jobs = {
            let mut statement = transaction.prepare(
                "SELECT backfill.source_id,backfill.acquisition_url,extraction.edition_id,extraction.work_id,
                        metadata_index.title,metadata_index.publication_year,metadata_index.authors_payload,
                        metadata_index.publishers_payload,metadata_index.isbns_payload
                 FROM catalog_metadata_backfill backfill
                 JOIN catalog_epub_extraction extraction
                   ON extraction.source_id=backfill.source_id AND extraction.acquisition_url=backfill.acquisition_url
                 JOIN catalog_source source ON source.source_id=backfill.source_id
                 JOIN catalog_publication publication
                   ON publication.source_id=backfill.source_id AND publication.generation=source.active_generation
                  AND publication.acquisition_url=backfill.acquisition_url
                 JOIN catalog_metadata_index metadata_index
                   ON metadata_index.source_id=backfill.source_id AND metadata_index.acquisition_url=backfill.acquisition_url
                  AND metadata_index.version=?1
                 WHERE backfill.status='pending' AND backfill.version=?2
                   AND extraction.status='succeeded'
                 ORDER BY backfill.attempts,backfill.source_id,backfill.acquisition_url LIMIT ?3",
            )?;
            let rows = statement.query_map(params![CATALOG_METADATA_INDEX_VERSION, CATALOG_METADATA_BACKFILL_VERSION, storage_i64(limit as u64, "metadata backfill batch size")?], |row| {
                let source = row.get::<_, String>(0)?;
                Ok(CatalogMetadataBackfillIndexedJob {
                    source_id: CatalogSourceId::parse(&source).map_err(|error| sql_decode_error(std::io::Error::new(std::io::ErrorKind::InvalidData, error)))?,
                    acquisition_url: row.get(1)?,
                    publication: CatalogPublicationIdentity { edition_id: row.get(2)?, work_id: row.get::<_, Option<String>>(3)?.filter(|value| !value.is_empty()) },
                    index: CatalogMetadataIndex {
                        title: row.get(4)?,
                        publication_year: row.get(5)?,
                        authors: wire::decode(&row.get::<_, Vec<u8>>(6)?, wire::MAX_DECODED_RESPONSE_BYTES).map_err(sql_decode_error)?,
                        publishers: wire::decode(&row.get::<_, Vec<u8>>(7)?, wire::MAX_DECODED_RESPONSE_BYTES).map_err(sql_decode_error)?,
                        isbns: wire::decode(&row.get::<_, Vec<u8>>(8)?, wire::MAX_DECODED_RESPONSE_BYTES).map_err(sql_decode_error)?,
                    },
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let now = unix_timestamp()?;
        for job in &jobs {
            transaction.execute(
                "UPDATE catalog_metadata_backfill SET status='running',attempts=attempts+1,last_attempt_at=?3,last_error=NULL WHERE source_id=?1 AND acquisition_url=?2 AND status='pending'",
                params![job.source_id.as_str(), job.acquisition_url, now],
            )?;
        }
        transaction.commit()?;
        Ok(jobs)
    }

    pub fn claim_metadata_backfill_batch(&self, limit: usize) -> Result<Vec<CatalogMetadataBackfillJob>, CatalogCacheError> {
        let limit = limit.clamp(1, 256);
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let jobs = {
            let mut statement = transaction.prepare(
                "SELECT backfill.source_id,backfill.acquisition_url,extraction.edition_id,extraction.work_id,extraction.metadata_payload
                 FROM catalog_metadata_backfill backfill
                 JOIN catalog_epub_extraction extraction
                   ON extraction.source_id=backfill.source_id AND extraction.acquisition_url=backfill.acquisition_url
                 JOIN catalog_source source ON source.source_id=backfill.source_id
                 JOIN catalog_publication publication
                   ON publication.source_id=backfill.source_id AND publication.generation=source.active_generation
                  AND publication.acquisition_url=backfill.acquisition_url
                 WHERE backfill.status='pending' AND backfill.version=?1
                   AND extraction.status='succeeded' AND extraction.metadata_payload IS NOT NULL
                 ORDER BY backfill.attempts,backfill.source_id,backfill.acquisition_url LIMIT ?2",
            )?;
            let rows = statement.query_map(params![CATALOG_METADATA_BACKFILL_VERSION, storage_i64(limit as u64, "metadata backfill batch size")?], |row| {
                let source = row.get::<_, String>(0)?;
                let edition_id = row.get(2)?;
                let work_id = row.get::<_, Option<String>>(3)?.filter(|value| !value.is_empty());
                let payload = row.get::<_, Vec<u8>>(4)?;
                let metadata = decode_cached(&payload).map_err(sql_decode_error)?;
                let source_id = CatalogSourceId::parse(&source).map_err(|error| sql_decode_error(std::io::Error::new(std::io::ErrorKind::InvalidData, error)))?;
                Ok(CatalogMetadataBackfillJob { source_id, publication: CatalogPublicationIdentity { edition_id, work_id }, acquisition_url: row.get(1)?, metadata })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let now = unix_timestamp()?;
        for job in &jobs {
            transaction.execute(
                "UPDATE catalog_metadata_backfill SET status='running',attempts=attempts+1,last_attempt_at=?3,last_error=NULL
                 WHERE source_id=?1 AND acquisition_url=?2 AND status='pending'",
                params![job.source_id.as_str(), job.acquisition_url, now],
            )?;
        }
        transaction.commit()?;
        Ok(jobs)
    }

    pub fn finish_metadata_backfill_batch(&self, updates: &[CatalogMetadataBackfillUpdate]) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for update in updates {
            let generation = active_generation_in(&transaction, &update.source_id)?;
            transaction.execute("UPDATE catalog_epub_extraction SET metadata_payload=?3 WHERE source_id=?1 AND acquisition_url=?2", params![update.source_id.as_str(), update.acquisition_url, wire::encode(&update.metadata)?])?;
            if sequence_in(&transaction, &update.source_id, generation, &update.publication)?.is_some() {
                merge_extracted_publication_authors(&transaction, &update.source_id, generation, &update.publication, &update.metadata)?;
                if let Some(mut active) = sequenced_publication_in(&transaction, &update.source_id, generation, &update.publication)? {
                    merge_catalog_subject_evidence(&mut active.publication, &update.subjects);
                    merge_identity_evidence(&mut active.publication, update.identity_result.as_ref());
                    upsert_active_publication(&transaction, &update.source_id, generation, &active.publication)?;
                    transaction.execute("UPDATE catalog_epub_extraction SET result_payload=?3 WHERE source_id=?1 AND acquisition_url=?2", params![update.source_id.as_str(), update.acquisition_url, wire::encode(&active.publication)?])?;
                }
                if let Some(popularity) = &update.popularity {
                    let work_id = update.publication.work_id.as_deref().unwrap_or("");
                    transaction.execute(
                        "INSERT OR REPLACE INTO catalog_publication_popularity(
                            source_id,generation,edition_id,work_id,want_to_read_count,currently_reading_count,already_read_count,reading_log_count,rating_count,rating_sum
                         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                        params![
                            update.source_id.as_str(),
                            storage_i64(generation, "generation")?,
                            update.publication.edition_id,
                            work_id,
                            storage_i64(popularity.want_to_read_count, "want-to-read count")?,
                            storage_i64(popularity.currently_reading_count, "currently-reading count")?,
                            storage_i64(popularity.already_read_count, "already-read count")?,
                            storage_i64(popularity.reading_log_count(), "reading-log count")?,
                            storage_i64(popularity.rating_count, "rating count")?,
                            storage_i64(popularity.rating_sum, "rating sum")?
                        ],
                    )?;
                }
            }
            match update.identity_result.as_ref() {
                Some(result) if result.status != metadata_contract::EditionIdentityStatus::Matched => {
                    let status = match result.status {
                        metadata_contract::EditionIdentityStatus::Ambiguous => "ambiguous",
                        metadata_contract::EditionIdentityStatus::NoMatch => "no_match",
                        metadata_contract::EditionIdentityStatus::InsufficientMetadata => "insufficient_metadata",
                        metadata_contract::EditionIdentityStatus::Matched => unreachable!(),
                    };
                    let review_id = blake3::hash(format!("{}\n{}", update.source_id.as_str(), update.acquisition_url).as_bytes()).to_hex().to_string();
                    let now = unix_timestamp()?;
                    transaction.execute(
                        "INSERT INTO catalog_identity_review(review_id,source_id,acquisition_url,status,result_payload,candidates_truncated,created_at,updated_at)
                         VALUES(?1,?2,?3,?4,?5,?6,?7,?7)
                         ON CONFLICT(source_id,acquisition_url) DO UPDATE SET
                           status=excluded.status,result_payload=excluded.result_payload,detail=NULL,candidates_truncated=excluded.candidates_truncated,
                           updated_at=excluded.updated_at,resolved_at=NULL,selected_isbn=NULL",
                        params![review_id, update.source_id.as_str(), update.acquisition_url, status, serde_json::to_vec(result)?, result.candidates_truncated, now],
                    )?;
                }
                _ => {
                    transaction.execute(
                        "DELETE FROM catalog_identity_review WHERE source_id=?1 AND acquisition_url=?2 AND status IN ('ambiguous','no_match','insufficient_metadata','failed')",
                        params![update.source_id.as_str(), update.acquisition_url],
                    )?;
                }
            }
            transaction.execute("DELETE FROM catalog_description_backfill WHERE source_id=?1 AND acquisition_url=?2", params![update.source_id.as_str(), update.acquisition_url])?;
            transaction.execute("UPDATE catalog_metadata_backfill SET status='succeeded',last_error=NULL WHERE source_id=?1 AND acquisition_url=?2 AND status='running'", params![update.source_id.as_str(), update.acquisition_url])?;
        }
        if !updates.is_empty() {
            advance_catalog_transport_revision(&transaction)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn finish_metadata_backfill_indexed_batch(&self, updates: &[CatalogMetadataBackfillIndexedUpdate]) -> Result<(), CatalogCacheError> {
        let prepared = updates.iter().map(|update| prepare_catalog_subject_assignments(&update.subjects)).collect::<Vec<_>>();
        self.finish_metadata_backfill_indexed_batch_timed(updates, &prepared).map(|_| ())
    }

    pub fn finish_metadata_backfill_indexed_batch_timed(&self, updates: &[CatalogMetadataBackfillIndexedUpdate], prepared_assignments: &[Vec<CatalogSubjectAssignment>]) -> Result<CatalogMetadataBackfillWriteTimings, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let transaction_started = Instant::now();
        let mut subjects_ms = 0;
        let mut authors_ms = 0;
        let mut identity_review_ms = 0;
        let mut index_ms = 0;
        let mut status_ms = 0;
        for (update, _prepared_assignments) in updates.iter().zip(prepared_assignments) {
            let generation = active_generation_in(&transaction, &update.source_id)?;
            if sequence_in(&transaction, &update.source_id, generation, &update.publication)?.is_some() {
                let authors_started = Instant::now();
                merge_extracted_author_names(&transaction, &update.source_id, generation, &update.publication, &update.index.authors)?;
                authors_ms += u64::try_from(authors_started.elapsed().as_millis()).unwrap_or(u64::MAX);
                let subjects_started = Instant::now();
                if let Some(mut active) = sequenced_publication_in(&transaction, &update.source_id, generation, &update.publication)? {
                    merge_catalog_subject_evidence(&mut active.publication, &update.subjects);
                    merge_identity_evidence(&mut active.publication, update.identity_result.as_ref());
                    upsert_active_publication(&transaction, &update.source_id, generation, &active.publication)?;
                    transaction.execute("UPDATE catalog_epub_extraction SET result_payload=?3 WHERE source_id=?1 AND acquisition_url=?2", params![update.source_id.as_str(), update.acquisition_url, wire::encode(&active.publication)?])?;
                }
                subjects_ms += u64::try_from(subjects_started.elapsed().as_millis()).unwrap_or(u64::MAX);
                if let Some(popularity) = &update.popularity {
                    let work_id = update.publication.work_id.as_deref().unwrap_or("");
                    transaction.execute(
                        "INSERT OR REPLACE INTO catalog_publication_popularity(source_id,generation,edition_id,work_id,want_to_read_count,currently_reading_count,already_read_count,reading_log_count,rating_count,rating_sum)
                         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                        params![
                            update.source_id.as_str(),
                            storage_i64(generation, "generation")?,
                            update.publication.edition_id,
                            work_id,
                            storage_i64(popularity.want_to_read_count, "want-to-read count")?,
                            storage_i64(popularity.currently_reading_count, "currently-reading count")?,
                            storage_i64(popularity.already_read_count, "already-read count")?,
                            storage_i64(popularity.reading_log_count(), "reading-log count")?,
                            storage_i64(popularity.rating_count, "rating count")?,
                            storage_i64(popularity.rating_sum, "rating sum")?
                        ],
                    )?;
                }
            }
            let review_started = Instant::now();
            record_identity_review(&transaction, update.source_id.as_str(), &update.acquisition_url, update.identity_result.as_ref())?;
            identity_review_ms += u64::try_from(review_started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let index_started = Instant::now();
            transaction.execute(
                "UPDATE catalog_metadata_index SET title=?3,publication_year=?4,authors_payload=?5,publishers_payload=?6,isbns_payload=?7 WHERE source_id=?1 AND acquisition_url=?2 AND version=?8",
                params![
                    update.source_id.as_str(),
                    update.acquisition_url,
                    update.index.title,
                    update.index.publication_year,
                    wire::encode(&update.index.authors)?,
                    wire::encode(&update.index.publishers)?,
                    wire::encode(&update.index.isbns)?,
                    CATALOG_METADATA_INDEX_VERSION
                ],
            )?;
            index_ms += u64::try_from(index_started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let status_started = Instant::now();
            transaction.execute("DELETE FROM catalog_description_backfill WHERE source_id=?1 AND acquisition_url=?2", params![update.source_id.as_str(), update.acquisition_url])?;
            transaction.execute("UPDATE catalog_metadata_backfill SET status='succeeded',last_error=NULL WHERE source_id=?1 AND acquisition_url=?2 AND status='running'", params![update.source_id.as_str(), update.acquisition_url])?;
            status_ms += u64::try_from(status_started.elapsed().as_millis()).unwrap_or(u64::MAX);
        }
        if !updates.is_empty() {
            advance_catalog_transport_revision(&transaction)?;
        }
        let transaction_ms = u64::try_from(transaction_started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let commit_started = Instant::now();
        transaction.commit()?;
        let commit_ms = u64::try_from(commit_started.elapsed().as_millis()).unwrap_or(u64::MAX);
        Ok(CatalogMetadataBackfillWriteTimings { subjects_ms, authors_ms, identity_review_ms, index_ms, status_ms, transaction_ms, commit_ms })
    }

    pub fn release_metadata_backfill_indexed_batch(&self, jobs: &[CatalogMetadataBackfillIndexedJob], error: &str) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let error = bounded_crawl_error(error);
        for job in jobs {
            transaction.execute("UPDATE catalog_metadata_backfill SET status='pending',last_error=?3 WHERE source_id=?1 AND acquisition_url=?2 AND status='running'", params![job.source_id.as_str(), job.acquisition_url, error])?;
            record_identity_review_failure(&transaction, job.source_id.as_str(), &job.acquisition_url, &error)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn release_metadata_backfill_batch(&self, jobs: &[CatalogMetadataBackfillJob], error: &str) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let error = bounded_crawl_error(error);
        for job in jobs {
            transaction.execute("UPDATE catalog_metadata_backfill SET status='pending',last_error=?3 WHERE source_id=?1 AND acquisition_url=?2 AND status='running'", params![job.source_id.as_str(), job.acquisition_url, error])?;
            let review_id = blake3::hash(format!("{}\n{}", job.source_id.as_str(), job.acquisition_url).as_bytes()).to_hex().to_string();
            let now = unix_timestamp()?;
            transaction.execute(
                "INSERT INTO catalog_identity_review(review_id,source_id,acquisition_url,status,detail,created_at,updated_at)
                 VALUES(?1,?2,?3,'failed',?4,?5,?5)
                 ON CONFLICT(source_id,acquisition_url) DO UPDATE SET
                   status='failed',result_payload=NULL,detail=excluded.detail,candidates_truncated=0,updated_at=excluded.updated_at,resolved_at=NULL,selected_isbn=NULL
                 WHERE catalog_identity_review.status NOT IN ('resolved','rejected')",
                params![review_id, job.source_id.as_str(), job.acquisition_url, error, now],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}

fn merge_identity_evidence(publication: &mut catalog_contract::CatalogPublication, result: Option<&metadata_contract::EditionIdentityResult>) {
    let Some(matched) = result.and_then(|result| result.matched.as_ref()) else { return };
    let evidence = [
        catalog_contract::Identifier::new(&matched.canonical_isbn13, catalog_contract::Scheme::Isbn, catalog_contract::Scope::Publication, "openlibrary:identity-match").expect("Open Library returned a bounded ISBN"),
        catalog_contract::Identifier::new(&matched.open_library_edition_id, catalog_contract::Scheme::Other("openlibrary".to_owned()), catalog_contract::Scope::Edition, "openlibrary:identity-match")
            .expect("Open Library returned a bounded edition identifier"),
    ];
    for identifier in evidence {
        if !publication.metadata.identifiers.contains(&identifier) {
            publication.metadata.identifiers.push(identifier);
        }
    }
    if let Some(work_id) = &matched.open_library_work_id {
        let identifier =
            catalog_contract::Identifier::new(work_id, catalog_contract::Scheme::Other("openlibrary".to_owned()), catalog_contract::Scope::Work, "openlibrary:identity-match").expect("Open Library returned a bounded work identifier");
        if !publication.metadata.identifiers.contains(&identifier) {
            publication.metadata.identifiers.push(identifier);
        }
    }
}
