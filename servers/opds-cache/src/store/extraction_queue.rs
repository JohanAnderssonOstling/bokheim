use super::{
    active_generation_in, active_generation_optional, advance_catalog_transport_revision, allocate_sequence, book_format_storage, bounded_crawl_error, catalog_book_format_from_storage, catalog_subject_evidence,
    merge_catalog_subject_evidence, normalize_search_text, params, prepare_catalog_subject_assignments, publication_extraction, publication_key, replace_publication_subjects_prepared, replace_publication_toc, sequenced_publication_in,
    sql_decode_error, storage_i64, unix_timestamp, upsert_active_publication, BTreeSet, CatalogCache, CatalogCacheError, CatalogEpubExtractionJob, CatalogEpubMetadata, CatalogMetadataIndex, CatalogPublication, CatalogPublicationIdentity,
    CatalogPublicationPopularity, CatalogSourceId, CatalogSubjectAssignment, HashSet, OptionalExtension, Transaction, TransactionBehavior, CATALOG_EPUB_ENRICHMENT_VERSION,
};
use publication_model::PublicationSubject;

pub(crate) fn queue_publication_extraction(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, publication: &CatalogPublication, force: bool) -> Result<(), CatalogCacheError> {
    let generation = storage_i64(generation, "generation")?;
    let work_id = publication.identity.work_id.as_deref().unwrap_or("");
    let extraction = publication_extraction(publication);
    transaction.execute(
        "DELETE FROM catalog_epub_extraction
         WHERE source_id=?1 AND edition_id=?2 AND work_id=?3
           AND acquisition_url IS NOT ?4",
        params![source_id.as_str(), publication.identity.edition_id, work_id, extraction.as_ref().map(|item| item.acquisition_url)],
    )?;
    let Some(extraction) = extraction else {
        return Ok(());
    };
    transaction.execute(
        "INSERT INTO catalog_epub_extraction(
             source_id, acquisition_url, status, enrichment_version,
             edition_id, work_id, book_format, cover_key, catalog_generation
         ) VALUES(?1, ?2, 'pending', ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(source_id, acquisition_url) DO UPDATE SET
             status=CASE
                 WHEN catalog_epub_extraction.status='running' THEN 'running'
                 WHEN ?9
                   OR catalog_epub_extraction.enrichment_version!=excluded.enrichment_version
                   OR catalog_epub_extraction.edition_id IS NOT excluded.edition_id
                   OR catalog_epub_extraction.work_id IS NOT excluded.work_id
                   OR catalog_epub_extraction.book_format IS NOT excluded.book_format
                   OR catalog_epub_extraction.cover_key IS NOT excluded.cover_key
                   OR (catalog_epub_extraction.status='succeeded' AND catalog_epub_extraction.result_payload IS NULL)
                 THEN 'pending' ELSE catalog_epub_extraction.status END,
             phase=CASE
                 WHEN catalog_epub_extraction.status='running' THEN catalog_epub_extraction.phase
                 WHEN ?9
                   OR catalog_epub_extraction.enrichment_version!=excluded.enrichment_version
                   OR catalog_epub_extraction.edition_id IS NOT excluded.edition_id
                   OR catalog_epub_extraction.work_id IS NOT excluded.work_id
                   OR catalog_epub_extraction.book_format IS NOT excluded.book_format
                   OR catalog_epub_extraction.cover_key IS NOT excluded.cover_key
                   OR catalog_epub_extraction.result_payload IS NULL
                 THEN NULL ELSE catalog_epub_extraction.phase END,
             last_error=CASE
                 WHEN ?9 OR catalog_epub_extraction.enrichment_version!=excluded.enrichment_version
                   OR catalog_epub_extraction.result_payload IS NULL
                 THEN NULL ELSE catalog_epub_extraction.last_error END,
             enrichment_version=excluded.enrichment_version,
             edition_id=excluded.edition_id,
             work_id=excluded.work_id,
             book_format=excluded.book_format,
             cover_key=excluded.cover_key,
             catalog_generation=excluded.catalog_generation",
        params![source_id.as_str(), extraction.acquisition_url, CATALOG_EPUB_ENRICHMENT_VERSION, publication.identity.edition_id, work_id, book_format_storage(extraction.book_format), publication.image_url, generation, force,],
    )?;
    Ok(())
}

pub(crate) fn merge_extracted_publication_authors(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, publication: &CatalogPublicationIdentity, metadata: &CatalogEpubMetadata) -> Result<(), CatalogCacheError> {
    let authors = metadata.book.authors().map(|author| author.name().to_owned()).collect::<Vec<_>>();
    merge_extracted_author_names(transaction, source_id, generation, publication, &authors)
}

pub(crate) fn merge_extracted_author_names(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, publication: &CatalogPublicationIdentity, authors: &[String]) -> Result<(), CatalogCacheError> {
    let generation = storage_i64(generation, "generation")?;
    let work_id = publication.work_id.as_deref().unwrap_or("");
    for (position, author) in authors.iter().enumerate() {
        let normalized_name = normalize_search_text(author);
        if normalized_name.is_empty() {
            continue;
        }
        transaction.execute(
            "INSERT OR IGNORE INTO catalog_publication_author(source_id,generation,edition_id,work_id,position,normalized_name) VALUES(?1,?2,?3,?4,?5,?6)",
            params![source_id.as_str(), generation, publication.edition_id, work_id, 1_000_000_i64 + i64::try_from(position).map_err(|_| CatalogCacheError::new("extracted author position overflow"))?, normalized_name],
        )?;
    }
    Ok(())
}

pub(crate) fn metadata_index_from_metadata(metadata: &CatalogEpubMetadata) -> CatalogMetadataIndex {
    CatalogMetadataIndex {
        title: metadata.book.title.clone(),
        authors: metadata.book.authors().map(|author| author.name().to_owned()).collect(),
        publishers: metadata.book.publication.publishers.iter().map(|publisher| publisher.name().as_str().to_owned()).collect(),
        isbns: metadata
            .book
            .publication
            .identifiers
            .iter()
            .filter(|identifier| identifier.scheme() == &publication_model::Scheme::Isbn)
            .filter_map(publication_model::Identifier::canonical_value)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        publication_year: publication_model::unique_publication_year(metadata.book.publication.dates.iter().map(|date| date.value())),
    }
}

impl CatalogCache {
    pub fn queue_crawls(&self, source_ids: &[CatalogSourceId]) -> Result<(), CatalogCacheError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for source_id in source_ids {
            transaction.execute("UPDATE catalog_source SET crawl_status='queued', last_crawl_error=NULL WHERE source_id=?1 AND crawl_status!='running'", [source_id.as_str()])?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn queue_epub_extractions(&self, source_id: &CatalogSourceId, acquisition_urls: &[String], force: bool) -> Result<(), CatalogCacheError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for acquisition_url in acquisition_urls {
            transaction.execute(
                "INSERT INTO catalog_epub_extraction(source_id, acquisition_url, status) VALUES(?1, ?2, 'pending')
                 ON CONFLICT(source_id, acquisition_url) DO UPDATE SET
                    status=CASE WHEN ?3 OR catalog_epub_extraction.enrichment_version!=?4 OR (catalog_epub_extraction.status='succeeded' AND catalog_epub_extraction.result_payload IS NULL) THEN 'pending' ELSE catalog_epub_extraction.status END,
                    phase=CASE WHEN ?3 OR catalog_epub_extraction.enrichment_version!=?4 OR catalog_epub_extraction.result_payload IS NULL THEN NULL ELSE catalog_epub_extraction.phase END,
                    last_error=CASE WHEN ?3 OR catalog_epub_extraction.enrichment_version!=?4 OR catalog_epub_extraction.result_payload IS NULL THEN NULL ELSE catalog_epub_extraction.last_error END",
                params![source_id.as_str(), acquisition_url, force, CATALOG_EPUB_ENRICHMENT_VERSION],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn retry_failed_epub_extractions(&self, source_id: &CatalogSourceId) -> Result<(), CatalogCacheError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE catalog_epub_extraction AS extraction SET status='pending',phase=NULL
             WHERE extraction.source_id=?1 AND extraction.status='failed'
               AND EXISTS(
                   SELECT 1 FROM catalog_epub_failure failure
                   WHERE failure.source_id=extraction.source_id AND failure.acquisition_url=extraction.acquisition_url
                     AND failure.retryable=1 AND COALESCE(failure.next_retry_at,0)<=?2
               )",
            params![source_id.as_str(), unix_timestamp()?],
        )?;
        Ok(())
    }

    /// Atomically claims one bounded job. Rows created by older binaries do
    /// not contain job metadata and are ignored until queue reconciliation
    /// backfills them from the active publication table.
    pub fn claim_next_epub_extraction(&self, source_id: &CatalogSourceId) -> Result<Option<CatalogEpubExtractionJob>, CatalogCacheError> {
        let connection = self.connection()?;
        let claimed = connection
            .query_row(
                "UPDATE catalog_epub_extraction
                 SET status='running', phase='downloading', attempts=attempts+1, last_attempt_at=?2, last_error=NULL
                 WHERE (source_id, acquisition_url)=(
                     SELECT source_id, acquisition_url FROM catalog_epub_extraction
                     WHERE source_id=?1 AND status='pending'
                       AND edition_id IS NOT NULL AND book_format IS NOT NULL
                       AND EXISTS(
                           SELECT 1 FROM catalog_publication publication
                           WHERE publication.source_id=catalog_epub_extraction.source_id
                             AND publication.generation=(SELECT active_generation FROM catalog_source WHERE source_id=?1)
                             AND publication.acquisition_url=catalog_epub_extraction.acquisition_url
                       )
                     ORDER BY attempts, acquisition_url LIMIT 1
                 ) AND status='pending'
                 RETURNING source_id, edition_id, work_id, acquisition_url, book_format, cover_key",
                params![source_id.as_str(), unix_timestamp()?],
                |row| {
                    let source_id = CatalogSourceId::parse(&row.get::<_, String>(0)?).map_err(|error| sql_decode_error(std::io::Error::new(std::io::ErrorKind::InvalidData, error)))?;
                    let edition_id = row.get(1)?;
                    let work_id = row.get::<_, Option<String>>(2)?.filter(|value| !value.is_empty());
                    let acquisition_url = row.get(3)?;
                    let format = row.get::<_, String>(4)?;
                    let book_format = catalog_book_format_from_storage(&format).ok_or_else(|| sql_decode_error(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("unsupported extraction book format {format}"))))?;
                    Ok(CatalogEpubExtractionJob { source_id, publication: CatalogPublicationIdentity { edition_id, work_id }, acquisition_url, book_format, cover_key: row.get(5)? })
                },
            )
            .optional()?;
        Ok(claimed)
    }

    pub fn begin_epub_extraction(&self, source_id: &CatalogSourceId, acquisition_url: &str) -> Result<bool, CatalogCacheError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE catalog_epub_extraction SET status='running', phase='downloading', attempts=attempts+1, last_attempt_at=?3, last_error=NULL WHERE source_id=?1 AND acquisition_url=?2 AND status IN ('pending','failed')",
            params![source_id.as_str(), acquisition_url, unix_timestamp()?],
        )?;
        Ok(changed == 1)
    }

    pub fn epub_extraction_status(&self, source_id: &CatalogSourceId, acquisition_url: &str) -> Result<Option<String>, CatalogCacheError> {
        let connection = self.connection()?;
        connection.query_row("SELECT status FROM catalog_epub_extraction WHERE source_id=?1 AND acquisition_url=?2", params![source_id.as_str(), acquisition_url], |row| row.get(0)).optional().map_err(Into::into)
    }

    pub fn set_epub_extraction_phase(&self, source_id: &CatalogSourceId, acquisition_url: &str, phase: &str) -> Result<(), CatalogCacheError> {
        if !matches!(phase, "downloading" | "extracting" | "enriching") {
            return Err(CatalogCacheError::new("invalid EPUB extraction phase"));
        }
        let connection = self.connection()?;
        connection.execute("UPDATE catalog_epub_extraction SET phase=?3 WHERE source_id=?1 AND acquisition_url=?2 AND status='running'", params![source_id.as_str(), acquisition_url, phase])?;
        Ok(())
    }

    pub fn finish_epub_extraction(&self, source_id: &CatalogSourceId, acquisition_url: &str, downloaded_bytes: u64, publication: &CatalogPublication, metadata: Option<&CatalogEpubMetadata>) -> Result<(), CatalogCacheError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "UPDATE catalog_epub_extraction SET status='succeeded', phase=NULL, downloaded_bytes=?3, last_succeeded_at=?4, last_error=NULL, result_payload=?5, metadata_payload=?6, enrichment_version=?7 WHERE source_id=?1 AND acquisition_url=?2",
            params![
                source_id.as_str(),
                acquisition_url,
                storage_i64(downloaded_bytes, "EPUB extraction downloaded bytes")?,
                unix_timestamp()?,
                wire::encode(publication)?,
                metadata.map(wire::encode).transpose()?,
                CATALOG_EPUB_ENRICHMENT_VERSION
            ],
        )?;
        transaction.execute("DELETE FROM catalog_epub_failure WHERE source_id=?1 AND acquisition_url=?2", params![source_id.as_str(), acquisition_url])?;
        if let Some(metadata) = metadata {
            if let Some(generation) = active_generation_optional(&transaction, source_id)? {
                merge_extracted_publication_authors(&transaction, source_id, generation, &publication.identity, metadata)?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn active_publication(&self, source_id: &CatalogSourceId, identity: &CatalogPublicationIdentity) -> Result<Option<CatalogPublication>, CatalogCacheError> {
        let connection = self.connection()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(None) };
        sequenced_publication_in(&connection, source_id, generation, identity).map(|publication| publication.map(|publication| publication.publication))
    }

    pub fn enrich_active_publication(
        &self, source_id: &CatalogSourceId, publication: &CatalogPublication, subjects: &[PublicationSubject], toc: &[publication_model::PublicationTocEntry], popularity: Option<&CatalogPublicationPopularity>,
    ) -> Result<bool, CatalogCacheError> {
        let mut publication = publication.clone();
        merge_catalog_subject_evidence(&mut publication, subjects);
        let combined_subjects = catalog_subject_evidence(&publication)?;
        let assignments = prepare_catalog_subject_assignments(&combined_subjects);
        self.enrich_active_publication_prepared(source_id, &publication, &combined_subjects, &assignments, toc, popularity)
    }

    pub fn enrich_active_publication_prepared(
        &self, source_id: &CatalogSourceId, publication: &CatalogPublication, subjects: &[PublicationSubject], assignments: &[CatalogSubjectAssignment], toc: &[publication_model::PublicationTocEntry],
        popularity: Option<&CatalogPublicationPopularity>,
    ) -> Result<bool, CatalogCacheError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = active_generation_in(&transaction, source_id)?;
        let (edition_id, work_id) = publication_key(publication);
        let exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4)",
            params![source_id.as_str(), storage_i64(generation, "generation")?, edition_id, work_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            return Ok(false);
        }
        upsert_active_publication(&transaction, source_id, generation, publication)?;
        // Completion can make a cached cover available without changing the
        // publication payload. Always advance the sequence so connected
        // clients refresh that card and retry its cover request.
        let completion_sequence = allocate_sequence(&transaction, source_id)?;
        transaction.execute(
            "UPDATE catalog_publication SET sequence=?5 WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, edition_id, work_id, completion_sequence],
        )?;
        replace_publication_subjects_prepared(&transaction, source_id, generation, &publication.identity, subjects, assignments)?;
        replace_publication_toc(&transaction, source_id, generation, &publication.identity, toc)?;
        if let Some(popularity) = popularity {
            transaction.execute(
                "INSERT OR REPLACE INTO catalog_publication_popularity(
                    source_id,generation,edition_id,work_id,want_to_read_count,currently_reading_count,already_read_count,reading_log_count,rating_count,rating_sum
                 ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    source_id.as_str(),
                    storage_i64(generation, "generation")?,
                    edition_id,
                    work_id,
                    storage_i64(popularity.want_to_read_count, "want-to-read count")?,
                    storage_i64(popularity.currently_reading_count, "currently-reading count")?,
                    storage_i64(popularity.already_read_count, "already-read count")?,
                    storage_i64(popularity.reading_log_count(), "reading-log count")?,
                    storage_i64(popularity.rating_count, "rating count")?,
                    storage_i64(popularity.rating_sum, "rating sum")?,
                ],
            )?;
        }
        advance_catalog_transport_revision(&transaction)?;
        transaction.commit()?;
        Ok(true)
    }

    /// Removes a definitively unusable cover without replacing the subject
    /// evidence produced by an earlier successful EPUB extraction. The saved
    /// extraction payload is updated too, so a later crawl cannot restore the
    /// rejected URL while overlaying persistent metadata.
    pub fn clear_active_publication_cover(&self, source_id: &CatalogSourceId, identity: &CatalogPublicationIdentity, acquisition_url: &str, expected_image_url: &str) -> Result<bool, CatalogCacheError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = active_generation_in(&transaction, source_id)?;
        let Some(mut publication) = sequenced_publication_in(&transaction, source_id, generation, identity)?.map(|publication| publication.publication) else { return Ok(false) };
        if publication.image_url.as_deref() != Some(expected_image_url) {
            return Ok(false);
        }
        publication.image_url = None;
        upsert_active_publication(&transaction, source_id, generation, &publication)?;
        let completion_sequence = allocate_sequence(&transaction, source_id)?;
        let (edition_id, work_id) = publication_key(&publication);
        transaction.execute(
            "UPDATE catalog_publication SET sequence=?5 WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, edition_id, work_id, completion_sequence],
        )?;
        transaction.execute("UPDATE catalog_epub_extraction SET result_payload=?3 WHERE source_id=?1 AND acquisition_url=?2", params![source_id.as_str(), acquisition_url, wire::encode(&publication)?])?;
        advance_catalog_transport_revision(&transaction)?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn fail_epub_extraction(&self, source_id: &CatalogSourceId, acquisition_url: &str, error: &str, category: &str, retryable: bool) -> Result<(), CatalogCacheError> {
        if category.is_empty() || category.len() > 64 || !category.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_') {
            return Err(CatalogCacheError::new("invalid EPUB failure category"));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let now = unix_timestamp()?;
        let attempts = transaction.query_row("SELECT attempts FROM catalog_epub_extraction WHERE source_id=?1 AND acquisition_url=?2", params![source_id.as_str(), acquisition_url], |row| row.get::<_, i64>(0))?;
        let retry_delay = match attempts {
            i64::MIN..=1 => 15 * 60,
            2 => 60 * 60,
            3 => 6 * 60 * 60,
            _ => 24 * 60 * 60,
        };
        let next_retry_at = retryable.then(|| now.saturating_add(retry_delay));
        transaction.execute("UPDATE catalog_epub_extraction SET status='failed', phase=NULL, last_error=?3 WHERE source_id=?1 AND acquisition_url=?2", params![source_id.as_str(), acquisition_url, bounded_crawl_error(error)])?;
        transaction.execute(
            "INSERT INTO catalog_epub_failure(source_id,acquisition_url,category,retryable,first_failed_at,last_failed_at,next_retry_at)
             VALUES(?1,?2,?3,?4,?5,?5,?6)
             ON CONFLICT(source_id,acquisition_url) DO UPDATE SET
                 category=excluded.category,retryable=excluded.retryable,last_failed_at=excluded.last_failed_at,next_retry_at=excluded.next_retry_at",
            params![source_id.as_str(), acquisition_url, category, retryable, now, next_retry_at],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn retain_epub_extractions(&self, source_id: &CatalogSourceId, retained_urls: &HashSet<String>) -> Result<(), CatalogCacheError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = {
            let mut statement = transaction.prepare("SELECT acquisition_url FROM catalog_epub_extraction WHERE source_id=?1")?;
            let rows = statement.query_map([source_id.as_str()], |row| row.get(0))?;
            rows.collect::<Result<Vec<String>, _>>()?
        };
        for acquisition_url in existing {
            if !retained_urls.contains(&acquisition_url) {
                transaction.execute("DELETE FROM catalog_epub_extraction WHERE source_id=?1 AND acquisition_url=?2", params![source_id.as_str(), acquisition_url])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }
}
