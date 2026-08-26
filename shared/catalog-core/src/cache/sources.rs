use super::{
    advance_catalog_transport_revision, decode_cached, decode_page, identity_review_inventory_in, optional_stored_u64, params, publication_entry_id, sql_decode_error, storage_i64, stored_u64, BTreeMap, CatalogCache, CatalogCacheError,
    CatalogCoverDownloadInventory, CatalogDescriptionBackfillInventory, CatalogEpubExtractionInventory, CatalogEpubFailureDiagnostic, CatalogEpubFailureExample, CatalogEpubMetadata, CatalogInventory, CatalogMetadataBackfillInventory,
    CatalogPublication, CatalogSourceId, CatalogSourceInventory, CatalogSourceState, Connection, OptionalExtension, Transaction, TransactionBehavior, Uuid, CATALOG_DESCRIPTION_BACKFILL_VERSION, CATALOG_SEARCH_INDEX_VERSION,
};
#[cfg(test)]
use super::{HashSet, CATALOG_EPUB_ENRICHMENT_VERSION};

impl CatalogCache {
    /// Rewrites cached publication payloads and page snapshots for one source.
    /// The publication sequence is intentionally preserved because this is a
    /// local presentation-data normalization, not a provider-side revision.
    pub fn rewrite_source_publications(&self, source_id: &CatalogSourceId, mut rewrite: impl FnMut(&mut CatalogPublication) -> bool) -> Result<usize, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let publications = {
            let mut statement = transaction.prepare("SELECT generation, edition_id, work_id, payload FROM catalog_publication WHERE source_id=?1")?;
            let rows = statement.query_map([source_id.as_str()], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Vec<u8>>(3)?)))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut changed = 0;
        for (generation, edition_id, work_id, payload) in publications {
            let mut publication: CatalogPublication = decode_cached(&payload)?;
            if rewrite(&mut publication) {
                let payload = wire::encode(&publication)?;
                let payload_hash = blake3::hash(&payload).to_hex().to_string();
                let entry_id = publication_entry_id(&transaction, source_id, &publication, &payload_hash)?;
                transaction.execute(
                    "UPDATE catalog_publication SET payload=?5, payload_hash=?6, entry_id=?7 WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
                    params![source_id.as_str(), generation, edition_id, work_id, payload, payload_hash, entry_id],
                )?;
                changed += 1;
            }
        }

        let pages = {
            let mut statement = transaction.prepare("SELECT generation, locator, payload FROM catalog_page WHERE source_id=?1")?;
            let rows = statement.query_map([source_id.as_str()], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Vec<u8>>(2)?)))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for (generation, locator, payload) in pages {
            let mut page = decode_page(&payload)?;
            let page_changed = page.publications.iter_mut().chain(page.groups.iter_mut().flat_map(|group| group.publications.iter_mut())).fold(false, |changed, publication| rewrite(publication) || changed);
            if page_changed {
                transaction.execute("UPDATE catalog_page SET payload=?4 WHERE source_id=?1 AND generation=?2 AND locator=?3", params![source_id.as_str(), generation, locator, wire::encode(&page)?])?;
            }
        }
        if changed > 0 {
            advance_catalog_transport_revision(&transaction)?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    pub fn inventory(&self) -> Result<CatalogInventory, CatalogCacheError> {
        let connection = self.pool.get()?;
        let mut statement = connection.prepare(
            "SELECT source_id, crawl_status, last_crawl_started_at, last_crawl_succeeded_at, last_crawl_error,
                    active_page_count, active_publication_count, active_epub_count,
                    (CASE WHEN NOT EXISTS(
                         SELECT 1 FROM catalog_page root
                         WHERE root.source_id=s.source_id AND root.generation=s.active_generation AND root.locator=s.source_url
                     ) THEN 1 ELSE 0 END) + (
                         SELECT COUNT(DISTINCT link.to_locator)
                         FROM catalog_page_link link
                         LEFT JOIN catalog_page page
                           ON page.source_id=link.source_id AND page.generation=link.generation AND page.locator=link.to_locator
                         WHERE link.source_id=s.source_id AND link.generation=s.active_generation AND page.locator IS NULL
                     ),
                    (SELECT COUNT(*) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id AND e.status='pending'),
                    (SELECT COUNT(*) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id AND e.status='running'),
                    (SELECT COUNT(*) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id AND e.status='running' AND e.phase='downloading'),
                    (SELECT COUNT(*) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id AND e.status='running' AND e.phase='extracting'),
                    (SELECT COUNT(*) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id AND e.status='running' AND e.phase='enriching'),
                    (SELECT COUNT(*) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id AND e.status='succeeded'),
                    (SELECT COUNT(*) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id AND e.status='failed'),
                    (SELECT COALESCE(SUM(attempts), 0) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id),
                    (SELECT COALESCE(SUM(downloaded_bytes), 0) FROM catalog_epub_extraction e WHERE e.source_id=s.source_id)
             FROM catalog_source s ORDER BY source_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(CatalogSourceInventory {
                source_id: CatalogSourceId::parse(&row.get::<_, String>(0)?).map_err(|error| sql_decode_error(std::io::Error::new(std::io::ErrorKind::InvalidData, error)))?,
                crawl_status: row.get(1)?,
                last_crawl_started_at: optional_stored_u64(row.get(2)?, "last crawl started at")?,
                last_crawl_succeeded_at: optional_stored_u64(row.get(3)?, "last crawl succeeded at")?,
                last_crawl_error: row.get(4)?,
                active_page_count: stored_u64(row.get(5)?, "active page count")?,
                active_publication_count: stored_u64(row.get(6)?, "active publication count")?,
                active_epub_count: stored_u64(row.get(7)?, "active EPUB count")?,
                crawl_frontier_count: stored_u64(row.get(8)?, "crawl frontier count")?,
                epub_extractions: CatalogEpubExtractionInventory {
                    pending: stored_u64(row.get(9)?, "pending EPUB extraction count")?,
                    running: stored_u64(row.get(10)?, "running EPUB extraction count")?,
                    downloading: stored_u64(row.get(11)?, "downloading publication count")?,
                    extracting: stored_u64(row.get(12)?, "extracting publication count")?,
                    enriching: stored_u64(row.get(13)?, "enriching publication count")?,
                    succeeded: stored_u64(row.get(14)?, "succeeded EPUB extraction count")?,
                    failed: stored_u64(row.get(15)?, "failed EPUB extraction count")?,
                    attempts: stored_u64(row.get(16)?, "EPUB extraction attempt count")?,
                    downloaded_bytes: stored_u64(row.get(17)?, "EPUB extraction downloaded bytes")?,
                },
            })
        })?;
        let sources = rows.collect::<Result<Vec<_>, _>>()?;
        let metadata_backfill = CatalogMetadataBackfillInventory {
            pending: connection.query_row("SELECT COUNT(*) FROM catalog_metadata_backfill WHERE status='pending'", [], |row| stored_u64(row.get(0)?, "pending metadata backfill count"))?,
            running: connection.query_row("SELECT COUNT(*) FROM catalog_metadata_backfill WHERE status='running'", [], |row| stored_u64(row.get(0)?, "running metadata backfill count"))?,
            succeeded: connection.query_row("SELECT COUNT(*) FROM catalog_metadata_backfill WHERE status='succeeded'", [], |row| stored_u64(row.get(0)?, "succeeded metadata backfill count"))?,
            last_error: connection.query_row("SELECT last_error FROM catalog_metadata_backfill WHERE last_error IS NOT NULL ORDER BY last_attempt_at DESC LIMIT 1", [], |row| row.get(0)).optional()?,
        };
        let description_backfill = CatalogDescriptionBackfillInventory {
            pending: connection
                .query_row("SELECT COUNT(*) FROM catalog_description_backfill WHERE version=?1 AND status='pending'", [CATALOG_DESCRIPTION_BACKFILL_VERSION], |row| stored_u64(row.get(0)?, "pending description backfill count"))?,
            running: connection
                .query_row("SELECT COUNT(*) FROM catalog_description_backfill WHERE version=?1 AND status='running'", [CATALOG_DESCRIPTION_BACKFILL_VERSION], |row| stored_u64(row.get(0)?, "running description backfill count"))?,
            succeeded: connection.query_row("SELECT COUNT(*) FROM catalog_description_backfill WHERE version=?1 AND status='succeeded' AND outcome IS NULL", [CATALOG_DESCRIPTION_BACKFILL_VERSION], |row| {
                stored_u64(row.get(0)?, "succeeded description backfill count")
            })?,
            no_description: connection.query_row("SELECT COUNT(*) FROM catalog_description_backfill WHERE version=?1 AND status='succeeded' AND outcome='no_description'", [CATALOG_DESCRIPTION_BACKFILL_VERSION], |row| {
                stored_u64(row.get(0)?, "Open Library entries without description count")
            })?,
            last_error: connection
                .query_row("SELECT last_error FROM catalog_description_backfill WHERE version=?1 AND last_error IS NOT NULL ORDER BY last_attempt_at DESC LIMIT 1", [CATALOG_DESCRIPTION_BACKFILL_VERSION], |row| row.get(0))
                .optional()?,
        };
        let identity_reviews = identity_review_inventory_in(&connection, None)?;
        let cover_downloads = CatalogCoverDownloadInventory {
            pending: connection.query_row("SELECT COUNT(*) FROM catalog_cover_download WHERE status='pending'", [], |row| stored_u64(row.get(0)?, "pending cover download count"))?,
            running: connection.query_row("SELECT COUNT(*) FROM catalog_cover_download WHERE status='running'", [], |row| stored_u64(row.get(0)?, "running cover download count"))?,
            succeeded: connection.query_row("SELECT COUNT(*) FROM catalog_cover_download WHERE status='succeeded'", [], |row| stored_u64(row.get(0)?, "succeeded cover download count"))?,
            failed: connection.query_row("SELECT COUNT(*) FROM catalog_cover_download WHERE status='failed'", [], |row| stored_u64(row.get(0)?, "failed cover download count"))?,
            attempts: connection.query_row("SELECT COALESCE(SUM(attempts),0) FROM catalog_cover_download", [], |row| stored_u64(row.get(0)?, "cover download attempt count"))?,
            last_error: connection.query_row("SELECT last_error FROM catalog_cover_download WHERE last_error IS NOT NULL ORDER BY last_attempt_at DESC LIMIT 1", [], |row| row.get(0)).optional()?,
        };
        Ok(CatalogInventory {
            search_index_ready: connection.query_row("SELECT index_version=?1 AND ready=1 FROM catalog_search_index_metadata WHERE singleton=1", [CATALOG_SEARCH_INDEX_VERSION], |row| row.get(0))?,
            active_page_count: sources.iter().map(|source| source.active_page_count).sum(),
            active_publication_count: sources.iter().map(|source| source.active_publication_count).sum(),
            active_epub_count: sources.iter().map(|source| source.active_epub_count).sum(),
            epub_extractions: sources.iter().fold(CatalogEpubExtractionInventory::default(), |mut total, source| {
                total.pending += source.epub_extractions.pending;
                total.running += source.epub_extractions.running;
                total.downloading += source.epub_extractions.downloading;
                total.extracting += source.epub_extractions.extracting;
                total.enriching += source.epub_extractions.enriching;
                total.succeeded += source.epub_extractions.succeeded;
                total.failed += source.epub_extractions.failed;
                total.attempts += source.epub_extractions.attempts;
                total.downloaded_bytes += source.epub_extractions.downloaded_bytes;
                total
            }),
            cover_downloads,
            metadata_backfill,
            description_backfill,
            identity_reviews,
            sources,
        })
    }

    /// Returns bounded, normalized diagnostics only. Raw acquisition URLs and
    /// raw upstream errors deliberately remain inside the protected cache.
    pub fn epub_failure_diagnostics(&self) -> Result<Vec<CatalogEpubFailureDiagnostic>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let mut statement = connection.prepare(
            "SELECT failure.category,failure.retryable,failure.first_failed_at,failure.last_failed_at,failure.next_retry_at,
                    extraction.source_id,extraction.acquisition_url,COALESCE(extraction.edition_id,''),extraction.attempts
             FROM catalog_epub_failure failure
             JOIN catalog_epub_extraction extraction
               ON extraction.source_id=failure.source_id AND extraction.acquisition_url=failure.acquisition_url
             WHERE extraction.status='failed'
             ORDER BY failure.category,failure.retryable,failure.last_failed_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, bool>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
            ))
        })?;
        let mut groups = BTreeMap::<(String, bool), CatalogEpubFailureDiagnostic>::new();
        for row in rows {
            let (category, retryable, first_failed_at, last_failed_at, next_retry_at, source_id, acquisition_url, publication_id, attempts) = row?;
            let key = (category.clone(), retryable);
            let entry = groups.entry(key).or_insert_with(|| CatalogEpubFailureDiagnostic { category, retryable, count: 0, first_failed_at: u64::MAX, last_failed_at: 0, next_retry_at: None, examples: Vec::new() });
            entry.count = entry.count.saturating_add(1);
            let first_failed_at = stored_u64(first_failed_at, "first extraction failure timestamp")?;
            let last_failed_at = stored_u64(last_failed_at, "last extraction failure timestamp")?;
            entry.first_failed_at = entry.first_failed_at.min(first_failed_at);
            entry.last_failed_at = entry.last_failed_at.max(last_failed_at);
            if let Some(next_retry_at) = next_retry_at {
                let next_retry_at = stored_u64(next_retry_at, "next extraction retry timestamp")?;
                entry.next_retry_at = Some(entry.next_retry_at.map_or(next_retry_at, |current| current.min(next_retry_at)));
            }
            if entry.examples.len() < 3 {
                let source_id = CatalogSourceId::parse(&source_id).map_err(CatalogCacheError::new)?;
                let acquisition_host = url::Url::parse(&acquisition_url).ok().and_then(|url| url.host_str().map(str::to_owned)).unwrap_or_else(|| "invalid-host".to_owned());
                entry.examples.push(CatalogEpubFailureExample { source_id, acquisition_host, publication_id, attempts: stored_u64(attempts, "extraction attempt count")?, last_failed_at });
            }
        }
        let mut diagnostics = groups.into_values().collect::<Vec<_>>();
        diagnostics.sort_by(|left, right| right.count.cmp(&left.count).then_with(|| left.category.cmp(&right.category)));
        Ok(diagnostics)
    }

    #[cfg(test)]
    pub fn successful_epub_extractions(&self, source_id: &CatalogSourceId) -> Result<HashSet<String>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let mut statement = connection.prepare("SELECT acquisition_url FROM catalog_epub_extraction WHERE source_id=?1 AND status='succeeded' AND result_payload IS NOT NULL AND enrichment_version=?2")?;
        let rows = statement.query_map(params![source_id.as_str(), CATALOG_EPUB_ENRICHMENT_VERSION], |row| row.get(0))?;
        rows.collect::<Result<HashSet<_>, _>>().map_err(Into::into)
    }

    #[cfg(test)]
    pub fn pending_epub_extractions(&self, source_id: &CatalogSourceId) -> Result<HashSet<String>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let mut statement = connection.prepare("SELECT acquisition_url FROM catalog_epub_extraction WHERE source_id=?1 AND status='pending'")?;
        let rows = statement.query_map([source_id.as_str()], |row| row.get(0))?;
        rows.collect::<Result<HashSet<_>, _>>().map_err(Into::into)
    }

    pub fn epub_metadata(&self, source_id: &CatalogSourceId, acquisition_url: &str) -> Result<Option<CatalogEpubMetadata>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let payload = connection
            .query_row("SELECT metadata_payload FROM catalog_epub_extraction WHERE source_id=?1 AND acquisition_url=?2 AND status='succeeded'", params![source_id.as_str(), acquisition_url], |row| row.get::<_, Option<Vec<u8>>>(0))
            .optional()?
            .flatten();
        payload.map(|payload| decode_cached(&payload)).transpose().map_err(Into::into)
    }
    pub fn source_state(&self, source_id: &CatalogSourceId) -> Result<Option<CatalogSourceState>, CatalogCacheError> {
        let connection = self.pool.get()?;
        connection
            .query_row("SELECT epoch, provider_epoch, active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| {
                Ok(CatalogSourceState { epoch: row.get(0)?, provider_epoch: row.get(1)?, active_generation: stored_u64(row.get(2)?, "active generation")? })
            })
            .optional()
            .map_err(Into::into)
    }

    /// Ensures a source exists. A URL change invalidates every canonical row
    /// for that source and creates a new epoch.
    pub fn ensure_source(&self, source_id: &CatalogSourceId, source_url: &str) -> Result<CatalogSourceState, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let state = source_state_in(&transaction, source_id)?.expect("source inserted");
        transaction.commit()?;
        Ok(state)
    }

    pub fn set_provider_epoch(&self, source_id: &CatalogSourceId, source_url: &str, provider_epoch: &str) -> Result<bool, CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let previous = transaction.query_row("SELECT provider_epoch FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| row.get::<_, Option<String>>(0))?;
        let changed = previous.as_deref().is_some_and(|previous| previous != provider_epoch);
        if changed {
            clear_source_rows(&transaction, source_id)?;
        }
        transaction.execute("UPDATE catalog_source SET provider_epoch=?2 WHERE source_id=?1", params![source_id.as_str(), provider_epoch])?;
        transaction.commit()?;
        Ok(changed)
    }

    pub fn remove_source(&self, source_id: &CatalogSourceId) -> Result<(), CatalogCacheError> {
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("DELETE FROM catalog_source WHERE source_id=?1", [source_id.as_str()])?;
        advance_catalog_transport_revision(&transaction)?;
        transaction.commit()?;
        Ok(())
    }
}

pub(crate) fn ensure_source_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId, source_url: &str) -> Result<(), CatalogCacheError> {
    let existing = transaction.query_row("SELECT source_url FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| row.get::<_, String>(0)).optional()?;
    match existing {
        Some(existing) if existing == source_url => {}
        Some(_) => {
            transaction.execute("DELETE FROM catalog_source WHERE source_id=?1", [source_id.as_str()])?;
            insert_source(transaction, source_id, source_url)?;
            advance_catalog_transport_revision(transaction)?;
        }
        None => insert_source(transaction, source_id, source_url)?,
    }
    Ok(())
}

pub(crate) fn insert_source(transaction: &Transaction<'_>, source_id: &CatalogSourceId, source_url: &str) -> Result<(), CatalogCacheError> {
    transaction.execute("INSERT INTO catalog_source(source_id, source_url, epoch, provider_epoch, active_generation, next_sequence) VALUES(?1, ?2, ?3, NULL, 1, 0)", params![source_id.as_str(), source_url, Uuid::new_v4().to_string()])?;
    Ok(())
}

pub(crate) fn clear_source_rows(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<(), CatalogCacheError> {
    transaction.execute("DELETE FROM catalog_page WHERE source_id=?1", [source_id.as_str()])?;
    transaction.execute("DELETE FROM catalog_publication WHERE source_id=?1", [source_id.as_str()])?;
    transaction.execute("DELETE FROM catalog_epub_extraction WHERE source_id=?1", [source_id.as_str()])?;
    transaction.execute(
        "UPDATE catalog_source SET epoch=?2, active_generation=1, next_sequence=0, crawl_status='never', last_crawl_error=NULL,
             active_page_count=0, active_publication_count=0, active_epub_count=0
         WHERE source_id=?1",
        params![source_id.as_str(), Uuid::new_v4().to_string()],
    )?;
    advance_catalog_transport_revision(transaction)?;
    Ok(())
}

pub(crate) fn refresh_active_counts(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64) -> Result<(), CatalogCacheError> {
    let generation = storage_i64(generation, "generation")?;
    transaction.execute(
        "UPDATE catalog_source
         SET active_page_count=(SELECT COUNT(*) FROM catalog_page WHERE source_id=?1 AND generation=?2),
             active_publication_count=(SELECT COUNT(*) FROM catalog_publication WHERE source_id=?1 AND generation=?2),
             active_epub_count=(SELECT COUNT(*) FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND book_format='epub' AND importable=1)
         WHERE source_id=?1",
        params![source_id.as_str(), generation],
    )?;
    Ok(())
}

pub(crate) fn source_state_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<Option<CatalogSourceState>, CatalogCacheError> {
    transaction
        .query_row("SELECT epoch, provider_epoch, active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| {
            Ok(CatalogSourceState { epoch: row.get(0)?, provider_epoch: row.get(1)?, active_generation: stored_u64(row.get(2)?, "active generation")? })
        })
        .optional()
        .map_err(Into::into)
}

pub(crate) fn source_state_connection(connection: &Connection, source_id: &CatalogSourceId) -> Result<Option<CatalogSourceState>, CatalogCacheError> {
    connection
        .query_row("SELECT epoch, provider_epoch, active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| {
            Ok(CatalogSourceState { epoch: row.get(0)?, provider_epoch: row.get(1)?, active_generation: stored_u64(row.get(2)?, "active generation")? })
        })
        .optional()
        .map_err(Into::into)
}

pub(crate) fn active_generation_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<u64, CatalogCacheError> {
    transaction.query_row("SELECT active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| stored_u64(row.get(0)?, "active generation")).map_err(Into::into)
}

pub(crate) fn active_generation_optional(connection: &Connection, source_id: &CatalogSourceId) -> Result<Option<u64>, CatalogCacheError> {
    connection.query_row("SELECT active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| stored_u64(row.get(0)?, "active generation")).optional().map_err(Into::into)
}

pub(crate) fn next_sequence_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<u64, CatalogCacheError> {
    transaction.query_row("SELECT next_sequence FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| stored_u64(row.get(0)?, "next sequence")).map_err(Into::into)
}
