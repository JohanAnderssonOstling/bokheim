use super::{
    active_generation_in, advance_catalog_transport_revision, bounded_crawl_error, merge_catalog_subject_evidence, params, sequence_in, sequenced_book_in, storage_i64, unix_timestamp, upsert_active_book, CatalogDescription,
    CatalogDescriptionBackfillJob, CatalogDescriptionBackfillUpdate, CatalogBookIdentity, CatalogSourceId, CatalogStore, CatalogStoreError, TransactionBehavior, CATALOG_DESCRIPTION_BACKFILL_VERSION,
    CATALOG_METADATA_BACKFILL_VERSION, CATALOG_METADATA_INDEX_VERSION,
};

impl CatalogStore {
    pub fn schedule_description_backfill(&self) -> Result<usize, CatalogStoreError> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO catalog_description_backfill(source_id,acquisition_url,version,status)
                 SELECT metadata_index.source_id,metadata_index.acquisition_url,?1,'pending'
                 FROM catalog_metadata_index metadata_index
                 JOIN catalog_epub_extraction extraction
                   ON extraction.source_id=metadata_index.source_id AND extraction.acquisition_url=metadata_index.acquisition_url
                 JOIN catalog_metadata_backfill metadata_backfill
                   ON metadata_backfill.source_id=metadata_index.source_id AND metadata_backfill.acquisition_url=metadata_index.acquisition_url
                  AND metadata_backfill.version=?3 AND metadata_backfill.status='succeeded'
                 JOIN catalog_source source ON source.source_id=metadata_index.source_id
                 JOIN catalog_book book
                   ON book.source_id=metadata_index.source_id AND book.generation=source.active_generation
                  AND book.acquisition_url=metadata_index.acquisition_url
                 WHERE metadata_index.version=?2 AND extraction.status='succeeded'
                   AND NOT EXISTS(
                       SELECT 1 FROM catalog_description_backfill current
                       WHERE current.source_id=metadata_index.source_id AND current.acquisition_url=metadata_index.acquisition_url AND current.version=?1
                   )
                 LIMIT 1000
                 ON CONFLICT(source_id,acquisition_url) DO UPDATE SET
                    version=excluded.version,status='pending',last_error=NULL,outcome=NULL
                 WHERE catalog_description_backfill.version!=excluded.version",
                params![CATALOG_DESCRIPTION_BACKFILL_VERSION, CATALOG_METADATA_INDEX_VERSION, CATALOG_METADATA_BACKFILL_VERSION],
            )
            .map_err(Into::into)
    }

    pub fn claim_description_backfill_batch(&self, limit: usize) -> Result<Vec<CatalogDescriptionBackfillJob>, CatalogStoreError> {
        let limit = limit.clamp(1, 256);
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let rows = {
            let mut statement = transaction.prepare(
                "SELECT backfill.source_id,backfill.acquisition_url,book.edition_id,book.work_id,
                        metadata_index.isbns_payload,backfill.attempts+1
                 FROM catalog_description_backfill backfill
                 JOIN catalog_source source ON source.source_id=backfill.source_id
                 JOIN catalog_book book
                   ON book.source_id=backfill.source_id AND book.generation=source.active_generation
                  AND book.acquisition_url=backfill.acquisition_url
                 JOIN catalog_metadata_index metadata_index
                   ON metadata_index.source_id=backfill.source_id AND metadata_index.acquisition_url=backfill.acquisition_url
                  AND metadata_index.version=?1
                 WHERE backfill.status='pending' AND backfill.version=?2
                 ORDER BY backfill.attempts,backfill.source_id,backfill.acquisition_url LIMIT ?3",
            )?;
            let rows = statement
                .query_map(params![CATALOG_METADATA_INDEX_VERSION, CATALOG_DESCRIPTION_BACKFILL_VERSION, storage_i64(limit as u64, "description backfill batch size")?], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?.filter(|value| !value.is_empty()), row.get::<_, Vec<u8>>(4)?, row.get::<_, i64>(5)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        let now = unix_timestamp()?;
        let mut jobs = Vec::new();
        for (source, acquisition_url, edition_id, work_id, isbns_payload, attempt) in rows {
            // Existing feed descriptions still need a lookup because this
            // isolated pass also applies validated Open Library subjects.
            let isbns: Vec<String> = wire::decode(&isbns_payload, wire::MAX_DECODED_RESPONSE_BYTES)?;
            let source_id = CatalogSourceId::parse(&source).map_err(|error| CatalogStoreError::new(error.to_string()))?;
            transaction.execute(
                "UPDATE catalog_description_backfill SET status='running',attempts=attempts+1,last_attempt_at=?3,last_error=NULL WHERE source_id=?1 AND acquisition_url=?2 AND status='pending'",
                params![source, acquisition_url, now],
            )?;
            jobs.push(CatalogDescriptionBackfillJob { source_id, book: CatalogBookIdentity { edition_id, work_id }, acquisition_url, isbns, attempt: super::stored_u64(attempt, "description backfill attempt")? });
        }
        transaction.commit()?;
        Ok(jobs)
    }

    pub fn finish_description_backfill_batch(&self, updates: &[CatalogDescriptionBackfillUpdate]) -> Result<(), CatalogStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for update in updates {
            let claimed = transaction.execute(
                "UPDATE catalog_description_backfill SET status='succeeded',last_error=NULL,outcome=?3 WHERE source_id=?1 AND acquisition_url=?2 AND status='running' AND attempts=?4",
                params![update.source_id.as_str(), update.acquisition_url, update.open_library_description_missing.then_some("no_description"), super::storage_i64(update.attempt, "description backfill attempt")?],
            )?;
            if claimed != 1 {
                continue;
            }
            let generation = active_generation_in(&transaction, &update.source_id)?;
            if sequence_in(&transaction, &update.source_id, generation, &update.book)?.is_some() {
                if let Some(mut active) = sequenced_book_in(&transaction, &update.source_id, generation, &update.book)? {
                    merge_catalog_subject_evidence(&mut active.book, &update.subjects);
                    if active.book.description.is_none() {
                        if let Some(description) = update.description.as_deref() {
                            active.book.description = Some(CatalogDescription::PlainText { text: description.to_owned() });
                        }
                    }
                    upsert_active_book(&transaction, &update.source_id, generation, &active.book)?;
                    transaction.execute("UPDATE catalog_epub_extraction SET result_payload=?3 WHERE source_id=?1 AND acquisition_url=?2", params![update.source_id.as_str(), update.acquisition_url, wire::encode(&active.book)?])?;
                }
            }
        }
        if !updates.is_empty() {
            advance_catalog_transport_revision(&transaction)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn release_description_backfill_batch(&self, jobs: &[CatalogDescriptionBackfillJob], error: &str) -> Result<(), CatalogStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let error = bounded_crawl_error(error);
        for job in jobs {
            transaction.execute(
                "UPDATE catalog_description_backfill SET status='pending',last_error=?3 WHERE source_id=?1 AND acquisition_url=?2 AND status='running' AND attempts=?4",
                params![job.source_id.as_str(), job.acquisition_url, error, super::storage_i64(job.attempt, "description backfill attempt")?],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}
