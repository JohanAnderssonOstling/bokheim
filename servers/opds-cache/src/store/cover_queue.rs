use super::{
    active_generation_in, advance_catalog_transport_revision, decode_cached, params, sql_decode_error, storage_i64, unix_timestamp, upsert_active_book, CatalogCoverDownloadJob, CatalogBook, CatalogBookIdentity,
    CatalogSourceId, CatalogStore, CatalogStoreError, HashSet, OptionalExtension, Transaction, TransactionBehavior,
};

const MAX_COVER_ERROR_BYTES: usize = 2_048;

impl CatalogStore {
    /// Reconciles the durable cover queue with active books in bounded
    /// transactions. The URL and both book identity components are
    /// retained together, so a cached image always has an auditable owner.
    pub fn reconcile_active_cover_downloads_batch(&self, batch_size: usize) -> Result<bool, CatalogStoreError> {
        let batch_size = i64::try_from(batch_size.clamp(1, 1_024)).map_err(|_| CatalogStoreError::new("cover reconciliation batch size overflow"))?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let rows = {
            let mut statement = transaction.prepare(
                "SELECT book.source_id,book.generation,book.payload
                 FROM catalog_book book
                 JOIN catalog_source source ON source.source_id=book.source_id AND source.active_generation=book.generation
                 WHERE (book.cover_url LIKE 'http://%' OR book.cover_url LIKE 'https://%')
                   AND NOT EXISTS(
                     SELECT 1 FROM catalog_cover_download cover
                     WHERE cover.source_id=book.source_id AND cover.edition_id=book.edition_id AND cover.work_id=book.work_id
                       AND cover.catalog_generation=book.generation
                 )
                 ORDER BY book.source_id,book.edition_id,book.work_id LIMIT ?1",
            )?;
            let rows = statement.query_map([batch_size], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, Vec<u8>>(2)?)))?.collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (source_id, generation, payload) in &rows {
            let book: CatalogBook = super::decode_cached(payload)?;
            let urls = book_cover_urls(&book);
            queue_cover_identity(&transaction, source_id, *generation, &book.identity.edition_id, book.identity.work_id.as_deref().unwrap_or(""), &urls)?;
        }
        let more = rows.len() == usize::try_from(batch_size).unwrap_or(usize::MAX);
        if !more {
            transaction.execute(
                "DELETE FROM catalog_cover_download AS cover
                 WHERE NOT EXISTS(
                     SELECT 1 FROM catalog_book book
                     JOIN catalog_source source ON source.source_id=book.source_id AND source.active_generation=book.generation
                     WHERE book.source_id=cover.source_id AND book.edition_id=cover.edition_id AND book.work_id=cover.work_id
                       AND book.generation=cover.catalog_generation
                 )",
                [],
            )?;
        }
        transaction.commit()?;
        Ok(more)
    }

    pub fn claim_next_cover_download(&self) -> Result<Option<CatalogCoverDownloadJob>, CatalogStoreError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "UPDATE catalog_cover_download
                 SET status='running', attempts=attempts+1, last_attempt_at=?1, last_error=NULL
                 WHERE (source_id,edition_id,work_id,image_url)=(
                     SELECT cover.source_id,cover.edition_id,cover.work_id,cover.image_url
                     FROM catalog_cover_download cover
                     JOIN catalog_source source ON source.source_id=cover.source_id
                     JOIN catalog_book book
                       ON book.source_id=cover.source_id AND book.generation=source.active_generation
                      AND book.edition_id=cover.edition_id AND book.work_id=cover.work_id
                     WHERE cover.status='pending' AND cover.catalog_generation=source.active_generation
                     ORDER BY cover.attempts,cover.priority,cover.source_id,cover.edition_id,cover.work_id,cover.image_url LIMIT 1
                 ) AND status='pending'
                 RETURNING source_id,edition_id,work_id,image_url",
                [unix_timestamp()?],
                |row| {
                    let source_id = CatalogSourceId::parse(&row.get::<_, String>(0)?).map_err(|error| sql_decode_error(std::io::Error::new(std::io::ErrorKind::InvalidData, error)))?;
                    let edition_id = row.get(1)?;
                    let work_id = row.get::<_, String>(2)?;
                    Ok(CatalogCoverDownloadJob { source_id, book: CatalogBookIdentity { edition_id, work_id: (!work_id.is_empty()).then_some(work_id) }, image_url: row.get(3)? })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn finish_cover_download(&self, job: &CatalogCoverDownloadJob, result: Result<(), &str>) -> Result<(), CatalogStoreError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let work_id = job.book.work_id.as_deref().unwrap_or("");
        let mut book_changed = false;
        match result {
            Ok(()) => {
                transaction.execute(
                    "UPDATE catalog_cover_download SET status='succeeded',last_succeeded_at=?5,last_error=NULL
                     WHERE source_id=?1 AND edition_id=?2 AND work_id=?3 AND image_url=?4 AND status='running'",
                    params![job.source_id.as_str(), job.book.edition_id, work_id, job.image_url, unix_timestamp()?],
                )?;
                let generation = active_generation_in(&transaction, &job.source_id)?;
                let payload = transaction
                    .query_row(
                        "SELECT payload FROM catalog_book WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
                        params![job.source_id.as_str(), storage_i64(generation, "generation")?, job.book.edition_id, work_id],
                        |row| row.get::<_, Vec<u8>>(0),
                    )
                    .optional()?;
                if let Some(payload) = payload {
                    let mut book: CatalogBook = decode_cached(&payload)?;
                    let current_failed = if let Some(url) = book.image_url.as_deref().filter(|url| url.starts_with("http://") || url.starts_with("https://")) {
                        transaction
                            .query_row(
                                "SELECT status='failed' FROM catalog_cover_download WHERE source_id=?1 AND edition_id=?2 AND work_id=?3 AND image_url=?4",
                                params![job.source_id.as_str(), job.book.edition_id, work_id, url],
                                |row| row.get::<_, bool>(0),
                            )
                            .optional()?
                            .unwrap_or(false)
                    } else {
                        false
                    };
                    if book.image_url.is_none() || current_failed {
                        book.image_url = Some(job.image_url.clone());
                        upsert_active_book(&transaction, &job.source_id, generation, &book)?;
                        book_changed = true;
                        transaction.execute(
                            "UPDATE catalog_epub_extraction SET result_payload=?4
                             WHERE source_id=?1 AND edition_id=?2 AND work_id=?3 AND result_payload IS NOT NULL",
                            params![job.source_id.as_str(), job.book.edition_id, work_id, wire::encode(&book)?],
                        )?;
                    }
                }
            }
            Err(error) => {
                transaction.execute(
                    "UPDATE catalog_cover_download SET status='failed',last_error=?5
                     WHERE source_id=?1 AND edition_id=?2 AND work_id=?3 AND image_url=?4 AND status='running'",
                    params![job.source_id.as_str(), job.book.edition_id, work_id, job.image_url, bounded_cover_error(error)],
                )?;
            }
        }
        if book_changed {
            advance_catalog_transport_revision(&transaction)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn retry_failed_cover_downloads(&self) -> Result<u64, CatalogStoreError> {
        let connection = self.connection()?;
        let changed = connection.execute("UPDATE catalog_cover_download SET status='pending',last_error=NULL WHERE status='failed'", [])?;
        u64::try_from(changed).map_err(|_| CatalogStoreError::new("cover retry count overflow"))
    }
}

pub(crate) fn queue_book_cover(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, book: &CatalogBook) -> Result<(), CatalogStoreError> {
    let work_id = book.identity.work_id.as_deref().unwrap_or("");
    let urls = book_cover_urls(book);
    queue_cover_identity(transaction, source_id.as_str(), storage_i64(generation, "generation")?, &book.identity.edition_id, work_id, &urls)
}

fn queue_cover_identity(transaction: &Transaction<'_>, source_id: &str, generation: i64, edition_id: &str, work_id: &str, image_urls: &[String]) -> Result<(), CatalogStoreError> {
    let expected = image_urls.iter().map(String::as_str).collect::<HashSet<_>>();
    let existing = {
        let mut statement = transaction.prepare("SELECT image_url FROM catalog_cover_download WHERE source_id=?1 AND edition_id=?2 AND work_id=?3")?;
        let rows = statement.query_map(params![source_id, edition_id, work_id], |row| row.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
        rows
    };
    for image_url in existing.into_iter().filter(|image_url| !expected.contains(image_url.as_str())) {
        transaction.execute("DELETE FROM catalog_cover_download WHERE source_id=?1 AND edition_id=?2 AND work_id=?3 AND image_url=?4", params![source_id, edition_id, work_id, image_url])?;
    }
    for (priority, image_url) in image_urls.iter().enumerate() {
        transaction.execute(
            "INSERT INTO catalog_cover_download(source_id,edition_id,work_id,image_url,priority,catalog_generation,status)
             VALUES(?1,?2,?3,?4,?5,?6,'pending')
             ON CONFLICT(source_id,edition_id,work_id,image_url) DO UPDATE SET priority=excluded.priority,catalog_generation=excluded.catalog_generation",
            params![source_id, edition_id, work_id, image_url, i64::try_from(priority).map_err(|_| CatalogStoreError::new("cover priority overflow"))?, generation],
        )?;
    }
    Ok(())
}

fn book_cover_urls(book: &CatalogBook) -> Vec<String> {
    let mut seen = HashSet::new();
    book.image_url.iter().chain(book.metadata.images.iter().map(|image| &image.url)).filter(|url| url.starts_with("http://") || url.starts_with("https://")).filter(|url| seen.insert((*url).clone())).cloned().collect()
}

fn bounded_cover_error(error: &str) -> String {
    let mut boundary = error.len().min(MAX_COVER_ERROR_BYTES);
    while !error.is_char_boundary(boundary) {
        boundary -= 1;
    }
    error[..boundary].to_owned()
}
