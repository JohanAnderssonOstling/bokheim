use super::{
    active_generation_in, bounded_crawl_error, copy_popularity_metadata, copy_subject_metadata, copy_toc_metadata, decode_cached, ensure_source_in, insert_staged_publication, next_sequence_in, params, put_page_row,
    queue_publication_extraction, refresh_active_counts, replace_publication_subjects, source_state_in, storage_i64, unix_timestamp, CatalogCache, CatalogCacheError, CatalogCrawlGeneration, CatalogPage, CatalogPageWrite,
    CatalogPublication, CatalogPublicationIdentity, CatalogSourceId, CatalogSourceState, OptionalExtension, PreparedCatalogPageWrite, TransactionBehavior,
};

impl CatalogCache {
    pub fn begin_crawl(&self, source_id: &CatalogSourceId, source_url: &str) -> Result<CatalogCrawlGeneration, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let active = active_generation_in(&transaction, source_id)?;
        let generation = active.checked_add(1).ok_or_else(|| CatalogCacheError::new("catalog generation overflow"))?;
        transaction.execute("DELETE FROM catalog_page WHERE source_id=?1 AND generation=?2", params![source_id.as_str(), storage_i64(generation, "generation")?])?;
        transaction.execute("DELETE FROM catalog_publication WHERE source_id=?1 AND generation=?2", params![source_id.as_str(), storage_i64(generation, "generation")?])?;
        transaction.execute("UPDATE catalog_source SET last_crawl_started_at=?2, crawl_status='running', last_crawl_error=NULL WHERE source_id=?1", params![source_id.as_str(), unix_timestamp()?])?;
        transaction.commit()?;
        Ok(CatalogCrawlGeneration { source_id: source_id.clone(), generation })
    }

    /// Starts or resumes discovery directly in the active generation. This is
    /// used for catalogues whose graph has never completed, so useful pages do
    /// not remain hidden behind an unbounded atomic crawl.
    pub fn begin_incremental_crawl(&self, source_id: &CatalogSourceId, source_url: &str) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        transaction.execute("UPDATE catalog_source SET last_crawl_started_at=?2, crawl_status='running', last_crawl_error=NULL WHERE source_id=?1", params![source_id.as_str(), unix_timestamp()?])?;
        transaction.commit()?;
        Ok(())
    }

    pub fn finish_incremental_crawl(&self, source_id: &CatalogSourceId, complete: bool) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = active_generation_in(&transaction, source_id)?;
        refresh_active_counts(&transaction, source_id, generation)?;
        if complete {
            transaction.execute("UPDATE catalog_source SET crawl_status='succeeded', last_crawl_succeeded_at=?2, last_crawl_error=NULL WHERE source_id=?1", params![source_id.as_str(), unix_timestamp()?])?;
        } else {
            transaction.execute("UPDATE catalog_source SET crawl_status='partial', last_crawl_error='crawl checkpoint reached; discovery will resume from the persisted frontier' WHERE source_id=?1", [source_id.as_str()])?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn stage_page(&self, crawl: &CatalogCrawlGeneration, page: &CatalogPage, etag: Option<&str>, last_modified: Option<&str>) -> Result<(), CatalogCacheError> {
        self.stage_prepared_pages(crawl, &Self::prepare_page_writes(vec![CatalogPageWrite { page: page.clone(), etag: etag.map(str::to_owned), last_modified: last_modified.map(str::to_owned) }])?)
    }

    /// Stages several replacement-crawl pages in one bounded transaction.
    /// Encoding and publication preparation remain outside BEGIN IMMEDIATE.
    pub fn stage_pages(&self, crawl: &CatalogCrawlGeneration, pages: &[CatalogPageWrite]) -> Result<(), CatalogCacheError> {
        self.stage_prepared_pages(crawl, &Self::prepare_page_writes(pages.to_vec())?)
    }

    pub fn stage_prepared_pages(&self, crawl: &CatalogCrawlGeneration, pages: &[PreparedCatalogPageWrite]) -> Result<(), CatalogCacheError> {
        if pages.is_empty() {
            return Ok(());
        }
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let active_generation = active_generation_in(&transaction, &crawl.source_id)?;
        for prepared_page in pages {
            put_page_row(&transaction, &crawl.source_id, crawl.generation, &prepared_page.write.page, &prepared_page.payload, prepared_page.write.etag.as_deref(), prepared_page.write.last_modified.as_deref())?;
            for publication in &prepared_page.publications {
                insert_staged_publication(&transaction, &crawl.source_id, crawl.generation, publication)?;
                copy_subject_metadata(&transaction, &crawl.source_id, active_generation, crawl.generation, &publication.publication.identity)?;
                copy_toc_metadata(&transaction, &crawl.source_id, active_generation, crawl.generation, &publication.publication.identity)?;
                copy_popularity_metadata(&transaction, &crawl.source_id, active_generation, crawl.generation, &publication.publication.identity)?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Replaces raw subject evidence for one staged publication and rebuilds
    /// its deterministic canonical assignments. This deliberately remains a
    /// sidecar to the catalogue wire payload: browsing clients do not need the
    /// potentially large raw metadata in every publication card.
    pub fn stage_publication_subjects(&self, crawl: &CatalogCrawlGeneration, identity: &CatalogPublicationIdentity, subjects: &[library_model::PublicationSubject]) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = storage_i64(crawl.generation, "generation")?;
        let work_id = identity.work_id.as_deref().unwrap_or("");
        let exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4)",
            params![crawl.source_id.as_str(), generation, identity.edition_id, work_id],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            return Err(CatalogCacheError::new("cannot attach subjects to a publication outside the staged generation"));
        }
        replace_publication_subjects(&transaction, &crawl.source_id, crawl.generation, identity, subjects)?;
        transaction.commit()?;
        Ok(())
    }
    pub fn activate_crawl(&self, crawl: &CatalogCrawlGeneration) -> Result<CatalogSourceState, CatalogCacheError> {
        while self.prepare_crawl_activation_batch(crawl, 256)? {}
        let state = self.finalize_crawl_activation(crawl)?;
        while self.reconcile_active_epub_extractions_batch(&crawl.source_id, 256)? {}
        Ok(state)
    }

    /// Assigns stable publication sequences to a bounded slice of a hidden
    /// generation. Each call commits independently so a large catalogue never
    /// monopolizes SQLite's single writer.
    pub fn prepare_crawl_activation_batch(&self, crawl: &CatalogCrawlGeneration, batch_size: usize) -> Result<bool, CatalogCacheError> {
        let batch_size = i64::try_from(batch_size.clamp(1, 1_024)).map_err(|_| CatalogCacheError::new("crawl activation batch size overflow"))?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let active = active_generation_in(&transaction, &crawl.source_id)?;
        let mut next_sequence = next_sequence_in(&transaction, &crawl.source_id)?;
        let generation = storage_i64(crawl.generation, "generation")?;
        let active_generation = storage_i64(active, "generation")?;
        let staged = {
            let mut statement = transaction.prepare(
                "SELECT edition_id,work_id,payload_hash FROM catalog_publication
                 WHERE source_id=?1 AND generation=?2 AND sequence=0
                 ORDER BY edition_id,work_id LIMIT ?3",
            )?;
            let rows = statement.query_map(params![crawl.source_id.as_str(), generation, batch_size], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?.collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (edition_id, work_id, payload_hash) in &staged {
            let previous_sequence = transaction
                .query_row(
                    "SELECT sequence FROM catalog_publication
                     WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 AND payload_hash=?5",
                    params![crawl.source_id.as_str(), active_generation, edition_id, work_id, payload_hash],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            let sequence = match previous_sequence {
                Some(sequence) => sequence,
                None => {
                    next_sequence = next_sequence.checked_add(1).ok_or_else(|| CatalogCacheError::new("publication sequence overflow"))?;
                    storage_i64(next_sequence, "publication sequence")?
                }
            };
            transaction.execute(
                "UPDATE catalog_publication SET sequence=?5
                 WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4 AND sequence=0",
                params![crawl.source_id.as_str(), generation, edition_id, work_id, sequence],
            )?;
        }
        transaction.execute("UPDATE catalog_source SET next_sequence=?2 WHERE source_id=?1", params![crawl.source_id.as_str(), storage_i64(next_sequence, "publication sequence")?])?;
        let remaining = staged.len() == usize::try_from(batch_size).unwrap_or(usize::MAX)
            && transaction.query_row("SELECT EXISTS(SELECT 1 FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND sequence=0)", params![crawl.source_id.as_str(), generation], |row| row.get::<_, bool>(0))?;
        transaction.commit()?;
        Ok(remaining)
    }

    /// Performs only the small atomic visibility switch after the hidden
    /// generation has been prepared in bounded commits.
    pub fn finalize_crawl_activation(&self, crawl: &CatalogCrawlGeneration) -> Result<CatalogSourceState, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = storage_i64(crawl.generation, "generation")?;
        let unprepared = transaction.query_row("SELECT EXISTS(SELECT 1 FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND sequence=0)", params![crawl.source_id.as_str(), generation], |row| row.get::<_, bool>(0))?;
        if unprepared {
            return Err(CatalogCacheError::new("crawl generation must be prepared before activation"));
        }

        let (page_count, publication_count, epub_count): (i64, i64, i64) = transaction.query_row(
            "SELECT
                 (SELECT COUNT(*) FROM catalog_page WHERE source_id=?1 AND generation=?2),
                 COUNT(*),
                 COALESCE(SUM(CASE WHEN book_format='epub' AND importable=1 THEN 1 ELSE 0 END), 0)
             FROM catalog_publication WHERE source_id=?1 AND generation=?2",
            params![crawl.source_id.as_str(), generation],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        transaction.execute(
            "UPDATE catalog_source
             SET active_generation=?2, last_crawl_succeeded_at=?3, crawl_status='succeeded', last_crawl_error=NULL,
                 active_page_count=?4, active_publication_count=?5, active_epub_count=?6
             WHERE source_id=?1",
            params![crawl.source_id.as_str(), generation, unix_timestamp()?, page_count, publication_count, epub_count],
        )?;
        let state = source_state_in(&transaction, &crawl.source_id)?.expect("crawl source exists");
        transaction.commit()?;
        Ok(state)
    }

    /// Reconciles a bounded slice of the durable extraction queue with the
    /// active generation. Stale jobs are removed only after every retained
    /// publication has been marked with the new generation.
    pub fn reconcile_active_epub_extractions_batch(&self, source_id: &CatalogSourceId, batch_size: usize) -> Result<bool, CatalogCacheError> {
        let batch_size = i64::try_from(batch_size.clamp(1, 1_024)).map_err(|_| CatalogCacheError::new("extraction reconciliation batch size overflow"))?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = active_generation_in(&transaction, source_id)?;
        let payloads = {
            let mut statement = transaction.prepare(
                "SELECT publication.payload
                 FROM catalog_publication publication
                 LEFT JOIN catalog_epub_extraction extraction
                   ON extraction.source_id=publication.source_id AND extraction.acquisition_url=publication.acquisition_url
                 WHERE publication.source_id=?1 AND publication.generation=?2
                   AND publication.acquisition_url IS NOT NULL AND publication.extraction_format IS NOT NULL
                   AND (extraction.acquisition_url IS NULL OR extraction.catalog_generation IS NOT ?2)
                 ORDER BY publication.edition_id,publication.work_id LIMIT ?3",
            )?;
            let rows = statement.query_map(params![source_id.as_str(), storage_i64(generation, "generation")?, batch_size], |row| row.get::<_, Vec<u8>>(0))?.collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for payload in &payloads {
            let publication: CatalogPublication = decode_cached(payload)?;
            queue_publication_extraction(&transaction, source_id, generation, &publication, false)?;
        }
        let more_publications = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM catalog_publication publication
                 LEFT JOIN catalog_epub_extraction extraction
                   ON extraction.source_id=publication.source_id AND extraction.acquisition_url=publication.acquisition_url
                 WHERE publication.source_id=?1 AND publication.generation=?2
                   AND publication.acquisition_url IS NOT NULL AND publication.extraction_format IS NOT NULL
                   AND (extraction.acquisition_url IS NULL OR extraction.catalog_generation IS NOT ?2)
             )",
            params![source_id.as_str(), storage_i64(generation, "generation")?],
            |row| row.get::<_, bool>(0),
        )?;
        let mut stale_removed = 0;
        if !more_publications {
            stale_removed = transaction.execute(
                "DELETE FROM catalog_epub_extraction WHERE rowid IN (
                     SELECT extraction.rowid FROM catalog_epub_extraction extraction
                     WHERE extraction.source_id=?1 AND NOT EXISTS(
                         SELECT 1 FROM catalog_publication publication
                         WHERE publication.source_id=extraction.source_id AND publication.generation=?2
                           AND publication.acquisition_url=extraction.acquisition_url
                     )
                     LIMIT ?3
                 )",
                params![source_id.as_str(), storage_i64(generation, "generation")?, batch_size],
            )?;
        }
        transaction.commit()?;
        Ok(more_publications || stale_removed == usize::try_from(batch_size).unwrap_or(usize::MAX))
    }

    /// Reclaims inactive generations without monopolizing SQLite's writer
    /// lock. Returns true while more rows remain for a later batch.
    pub fn cleanup_inactive_generation_batch(&self, source_id: &CatalogSourceId, batch_size: usize) -> Result<bool, CatalogCacheError> {
        let batch_size = i64::try_from(batch_size.max(1)).map_err(|_| CatalogCacheError::new("cleanup batch size overflow"))?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let active = storage_i64(active_generation_in(&transaction, source_id)?, "generation")?;
        transaction.execute(
            "DELETE FROM catalog_page WHERE rowid IN (
                 SELECT rowid FROM catalog_page WHERE source_id=?1 AND generation<>?2 LIMIT ?3
             )",
            params![source_id.as_str(), active, batch_size],
        )?;
        transaction.execute(
            "DELETE FROM catalog_publication WHERE rowid IN (
                 SELECT rowid FROM catalog_publication WHERE source_id=?1 AND generation<>?2 LIMIT ?3
             )",
            params![source_id.as_str(), active, batch_size],
        )?;
        let remaining = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM catalog_page WHERE source_id=?1 AND generation<>?2)
                 OR EXISTS(SELECT 1 FROM catalog_publication WHERE source_id=?1 AND generation<>?2)",
            params![source_id.as_str(), active],
            |row| row.get::<_, bool>(0),
        )?;
        transaction.commit()?;
        Ok(remaining)
    }

    pub fn abandon_crawl(&self, crawl: &CatalogCrawlGeneration) -> Result<(), CatalogCacheError> {
        self.abandon_crawl_with_error(crawl, None)
    }

    pub fn abandon_crawl_with_error(&self, crawl: &CatalogCrawlGeneration, error: Option<&str>) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let error = error.map(bounded_crawl_error);
        transaction.execute("UPDATE catalog_source SET crawl_status='failed', last_crawl_error=?2 WHERE source_id=?1", params![crawl.source_id.as_str(), error])?;
        transaction.commit()?;
        Ok(())
    }
}
