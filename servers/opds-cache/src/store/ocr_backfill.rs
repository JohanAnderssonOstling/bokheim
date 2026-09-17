use super::{
    active_generation_in, advance_catalog_transport_revision, allocate_sequence, catalog_book_format_from_storage, decode_cached, params, replace_book_subjects_prepared, sql_decode_error, storage_i64, stored_u64, unix_timestamp,
    upsert_active_book, CatalogOcrBackfillJob, CatalogBook, CatalogBookIdentity, CatalogSourceId, CatalogStore, CatalogStoreError, CatalogSubjectAssignment, OptionalExtension, TransactionBehavior,
    CATALOG_METADATA_BACKFILL_VERSION,
};
use std::io;

impl CatalogStore {
    pub fn claim_ocr_backfill_batch(&self, limit: usize) -> Result<Vec<CatalogOcrBackfillJob>, CatalogStoreError> {
        let limit = limit.clamp(1, 16);
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let jobs = {
            let mut statement = transaction.prepare(
                "SELECT backfill.source_id,backfill.acquisition_url,extraction.edition_id,extraction.work_id,extraction.book_format,backfill.ocr_attempts+1
                   FROM catalog_metadata_backfill backfill
                   JOIN catalog_epub_extraction extraction
                     ON extraction.source_id=backfill.source_id AND extraction.acquisition_url=backfill.acquisition_url
                   JOIN catalog_source source ON source.source_id=backfill.source_id
                  JOIN catalog_book book
                     ON book.source_id=backfill.source_id AND book.generation=source.active_generation
                    AND book.acquisition_url=backfill.acquisition_url
                  WHERE backfill.version=?1 AND backfill.status='succeeded' AND backfill.ocr_status='pending'
                    AND extraction.status='succeeded' AND extraction.metadata_payload IS NOT NULL
                    AND NOT EXISTS(
                        SELECT 1 FROM catalog_epub_extraction pending_extraction
                         WHERE pending_extraction.source_id=backfill.source_id
                           AND pending_extraction.status IN ('pending','running')
                    )
                    AND NOT EXISTS(
                        SELECT 1 FROM catalog_metadata_backfill pending_metadata
                         WHERE pending_metadata.source_id=backfill.source_id
                           AND pending_metadata.status IN ('pending','running')
                    )
                  ORDER BY backfill.ocr_attempts,backfill.source_id,backfill.acquisition_url LIMIT ?2",
            )?;
            let rows = statement.query_map(params![CATALOG_METADATA_BACKFILL_VERSION, storage_i64(limit as u64, "OCR backfill batch size")?], |row| {
                let source = row.get::<_, String>(0)?;
                let format = row.get::<_, String>(4)?;
                Ok(CatalogOcrBackfillJob {
                    source_id: CatalogSourceId::parse(&source).map_err(|error| sql_decode_error(io::Error::new(io::ErrorKind::InvalidData, error)))?,
                    acquisition_url: row.get(1)?,
                    book: CatalogBookIdentity { edition_id: row.get(2)?, work_id: row.get::<_, Option<String>>(3)?.filter(|value| !value.is_empty()) },
                    book_format: catalog_book_format_from_storage(&format).ok_or_else(|| sql_decode_error(io::Error::new(io::ErrorKind::InvalidData, format!("unsupported OCR book format {format}"))))?,
                    attempt: stored_u64(row.get(5)?, "OCR backfill attempt")?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let now = unix_timestamp()?;
        for job in &jobs {
            transaction.execute(
                "UPDATE catalog_metadata_backfill
                    SET ocr_status='running',ocr_attempts=ocr_attempts+1,ocr_last_attempt_at=?3,ocr_last_error=NULL
                  WHERE source_id=?1 AND acquisition_url=?2 AND ocr_status='pending'",
                params![job.source_id.as_str(), job.acquisition_url, now],
            )?;
        }
        transaction.commit()?;
        Ok(jobs)
    }

    pub fn mark_ocr_not_needed(&self, source_id: &CatalogSourceId, acquisition_url: &str, attempt: u64) -> Result<(), CatalogStoreError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE catalog_metadata_backfill SET ocr_status='not_needed',ocr_last_error=NULL WHERE source_id=?1 AND acquisition_url=?2 AND ocr_status='running' AND ocr_attempts=?3",
            params![source_id.as_str(), acquisition_url, storage_i64(attempt, "OCR backfill attempt")?],
        )?;
        Ok(())
    }

    pub fn finish_ocr_backfill(
        &self, source_id: &CatalogSourceId, acquisition_url: &str, book: &CatalogBook, subjects: &[book_model::BookSubject], assignments: &[CatalogSubjectAssignment], attempt: u64,
    ) -> Result<bool, CatalogStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let generation = active_generation_in(&transaction, source_id)?;
        let current_acquisition_url = transaction
            .query_row(
                "SELECT acquisition_url FROM catalog_book WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
                params![source_id.as_str(), storage_i64(generation, "generation")?, book.identity.edition_id, book.identity.work_id.as_deref().unwrap_or("")],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if current_acquisition_url.as_deref() != Some(acquisition_url) {
            transaction.execute(
                "UPDATE catalog_metadata_backfill SET ocr_status='not_needed',ocr_last_error='book is no longer active' WHERE source_id=?1 AND acquisition_url=?2 AND ocr_status='running' AND ocr_attempts=?3",
                params![source_id.as_str(), acquisition_url, storage_i64(attempt, "OCR backfill attempt")?],
            )?;
            transaction.commit()?;
            return Ok(false);
        }
        let claimed = transaction.execute(
            "UPDATE catalog_metadata_backfill SET ocr_status='succeeded',ocr_last_error=NULL WHERE source_id=?1 AND acquisition_url=?2 AND ocr_status='running' AND ocr_attempts=?3",
            params![source_id.as_str(), acquisition_url, storage_i64(attempt, "OCR backfill attempt")?],
        )?;
        if claimed != 1 {
            transaction.commit()?;
            return Ok(false);
        }
        let current_payload = transaction.query_row(
            "SELECT payload FROM catalog_book WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, book.identity.edition_id, book.identity.work_id.as_deref().unwrap_or("")],
            |row| row.get::<_, Vec<u8>>(0),
        )?;
        let mut current_book: CatalogBook = decode_cached(&current_payload)?;
        current_book.metadata.subjects = book.metadata.subjects.clone();
        upsert_active_book(&transaction, source_id, generation, &current_book)?;
        let sequence = allocate_sequence(&transaction, source_id)?;
        let identity = &current_book.identity;
        transaction.execute(
            "UPDATE catalog_book SET sequence=?5 WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, identity.edition_id, identity.work_id.as_deref().unwrap_or(""), sequence],
        )?;
        replace_book_subjects_prepared(&transaction, source_id, generation, identity, subjects, assignments)?;
        transaction.execute("UPDATE catalog_epub_extraction SET result_payload=?3 WHERE source_id=?1 AND acquisition_url=?2", params![source_id.as_str(), acquisition_url, wire::encode(&current_book)?])?;
        advance_catalog_transport_revision(&transaction)?;
        transaction.commit()?;
        Ok(true)
    }

    pub fn release_ocr_backfill(&self, jobs: &[CatalogOcrBackfillJob], error: &str) -> Result<(), CatalogStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let error = super::bounded_crawl_error(error);
        for job in jobs {
            transaction.execute(
                "UPDATE catalog_metadata_backfill SET ocr_status='pending',ocr_last_error=?3 WHERE source_id=?1 AND acquisition_url=?2 AND ocr_status='running' AND ocr_attempts=?4",
                params![job.source_id.as_str(), job.acquisition_url, error, storage_i64(job.attempt, "OCR backfill attempt")?],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }
}
