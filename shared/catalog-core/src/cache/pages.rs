use super::{
    active_generation_in, active_generation_optional, ensure_source_in, materialize_page, next_sequence_in, normalize_search_text, page_layout, params, publication_extraction, queue_publication_extraction, refresh_active_counts,
    source_state_connection, upsert_active_prepared_publication, upsert_publication_with_sequence, CachedCatalogPage, CatalogCache, CatalogCacheError, CatalogDescription, CatalogLinkTarget, CatalogPage, CatalogPageResponse,
    CatalogPageWrite, CatalogPublication, CatalogPublicationIdentity, CatalogPublicationPayload, CatalogPublicationReference, CatalogSearchMatches, CatalogSearchResponse, CatalogSourceId, Connection, HashMap, HashSet, OptionalExtension,
    PreparedCatalogPageWrite, SequencedCatalogPublication, Transaction, TransactionBehavior,
};

impl CatalogCache {
    /// Stores a directly fetched page into the active local generation.
    pub fn put_page(&self, source_id: &CatalogSourceId, source_url: &str, page: &CatalogPage, etag: Option<&str>, last_modified: Option<&str>) -> Result<(), CatalogCacheError> {
        let prepared = Self::prepare_page_writes(vec![CatalogPageWrite { page: page.clone(), etag: etag.map(str::to_owned), last_modified: last_modified.map(str::to_owned) }])?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let generation = active_generation_in(&transaction, source_id)?;
        put_prepared_page_rows(&transaction, source_id, generation, &prepared)?;
        refresh_active_counts(&transaction, source_id, generation)?;
        transaction.commit()?;
        Ok(())
    }

    /// Performs all CPU-only preparation needed by a page batch. Callers that
    /// serialize writes can run this before entering their global writer queue.
    pub fn prepare_page_writes(pages: Vec<CatalogPageWrite>) -> Result<Vec<PreparedCatalogPageWrite>, CatalogCacheError> {
        pages.into_iter().map(PreparedCatalogPageWrite::new).collect()
    }

    /// Commits a bounded set of incrementally discovered pages together.
    /// Inventory counts deliberately remain stale until finish_incremental_crawl.
    pub fn put_pages(&self, source_id: &CatalogSourceId, source_url: &str, pages: &[CatalogPageWrite]) -> Result<(), CatalogCacheError> {
        self.put_prepared_pages(source_id, source_url, &Self::prepare_page_writes(pages.to_vec())?)
    }

    pub fn put_prepared_pages(&self, source_id: &CatalogSourceId, source_url: &str, pages: &[PreparedCatalogPageWrite]) -> Result<(), CatalogCacheError> {
        if pages.is_empty() {
            return Ok(());
        }
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let generation = active_generation_in(&transaction, source_id)?;
        put_prepared_page_rows(&transaction, source_id, generation, pages)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn page(&self, source_id: &CatalogSourceId, locator: &str) -> Result<Option<CachedCatalogPage>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else {
            return Ok(None);
        };
        connection
            .query_row("SELECT payload, etag, last_modified FROM catalog_page WHERE source_id=?1 AND generation=?2 AND locator=?3", params![source_id.as_str(), storage_i64(generation, "generation")?, locator], |row| {
                let payload: Vec<u8> = row.get(0)?;
                let page = decode_page(&payload).map_err(sql_decode_error)?;
                Ok(CachedCatalogPage { page, etag: row.get(1)?, last_modified: row.get(2)? })
            })
            .optional()
            .map_err(Into::into)
    }

    pub fn has_pages(&self, source_id: &CatalogSourceId) -> Result<bool, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(false) };
        connection.query_row("SELECT EXISTS(SELECT 1 FROM catalog_page WHERE source_id=?1 AND generation=?2)", params![source_id.as_str(), storage_i64(generation, "generation")?], |row| row.get(0)).map_err(Into::into)
    }

    /// Checks for unfinished graph discovery without materializing every
    /// missing locator. The crawler walks cached pages into its own temporary
    /// disk frontier when this returns true.
    pub fn has_missing_page_frontier(&self, source_id: &CatalogSourceId, root_locator: &str) -> Result<bool, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(true) };
        let generation = storage_i64(generation, "generation")?;
        let root_exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM catalog_page WHERE source_id=?1 AND generation=?2 AND locator=?3)", params![source_id.as_str(), generation, root_locator], |row| row.get::<_, bool>(0))?;
        if !root_exists {
            return Ok(true);
        }
        connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM catalog_page_link link
                     LEFT JOIN catalog_page page
                       ON page.source_id=link.source_id AND page.generation=link.generation AND page.locator=link.to_locator
                     WHERE link.source_id=?1 AND link.generation=?2 AND page.locator IS NULL
                 )",
                params![source_id.as_str(), generation],
                |row| row.get(0),
            )
            .map_err(Into::into)
    }

    /// Returns only links whose target page is absent from the active
    /// generation. Page payloads remain in SQLite.
    pub fn missing_page_frontier(&self, source_id: &CatalogSourceId, root_locator: &str) -> Result<Vec<String>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(vec![root_locator.to_owned()]) };
        let generation = storage_i64(generation, "generation")?;
        let mut frontier = Vec::new();
        let root_exists = connection.query_row("SELECT EXISTS(SELECT 1 FROM catalog_page WHERE source_id=?1 AND generation=?2 AND locator=?3)", params![source_id.as_str(), generation, root_locator], |row| row.get::<_, bool>(0))?;
        if !root_exists {
            frontier.push(root_locator.to_owned());
        }
        let mut statement = connection.prepare(
            "SELECT DISTINCT link.to_locator
             FROM catalog_page_link link
             LEFT JOIN catalog_page page
               ON page.source_id=link.source_id AND page.generation=link.generation AND page.locator=link.to_locator
             WHERE link.source_id=?1 AND link.generation=?2 AND page.locator IS NULL
             ORDER BY link.to_locator",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), generation], |row| row.get::<_, String>(0))?;
        frontier.extend(rows.collect::<Result<Vec<_>, _>>()?);
        Ok(frontier)
    }

    pub fn enrich_page_from_epub_extractions(&self, source_id: &CatalogSourceId, page: &mut CatalogPage) -> Result<(), CatalogCacheError> {
        let connection = self.pool.get()?;
        let mut statement = connection.prepare(
            "SELECT result_payload FROM catalog_epub_extraction
             WHERE source_id=?1 AND acquisition_url=?2 AND status='succeeded' AND result_payload IS NOT NULL",
        )?;
        for publication in page.publications.iter_mut().chain(page.groups.iter_mut().flat_map(|group| group.publications.iter_mut())) {
            let Some(extraction) = publication_extraction(publication) else { continue };
            let payload = statement.query_row(params![source_id.as_str(), extraction.acquisition_url], |row| row.get::<_, Vec<u8>>(0)).optional()?;
            let Some(payload) = payload else { continue };
            let enriched: CatalogPublication = decode_cached(&payload)?;
            publication.title = enriched.title;
            publication.authors = enriched.authors;
            publication.languages = enriched.languages;
            publication.description = enriched.description;
            publication.image_url = enriched.image_url;
        }
        Ok(())
    }

    pub fn active_publication_for_cover(&self, source_id: &CatalogSourceId, cover_url: &str) -> Result<Option<CatalogPublication>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else { return Ok(None) };
        let payload = connection
            .query_row(
                "SELECT payload FROM catalog_publication
                 WHERE source_id=?1 AND generation=?2 AND cover_url=?3
                 ORDER BY edition_id,work_id LIMIT 1",
                params![source_id.as_str(), storage_i64(generation, "generation")?, cover_url],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?;
        payload.map(|payload| decode_cached(&payload)).transpose().map_err(Into::into)
    }

    pub fn pages(&self, source_id: &CatalogSourceId) -> Result<Vec<CachedCatalogPage>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else {
            return Ok(Vec::new());
        };
        let mut statement = connection.prepare(
            "SELECT payload, etag, last_modified FROM catalog_page
             WHERE source_id=?1 AND generation=?2 ORDER BY locator",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), storage_i64(generation, "generation")?], |row| {
            let payload: Vec<u8> = row.get(0)?;
            let page = decode_page(&payload).map_err(sql_decode_error)?;
            Ok(CachedCatalogPage { page, etag: row.get(1)?, last_modified: row.get(2)? })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn known_page_publications(&self, source_id: &CatalogSourceId, locator: &str) -> Result<Vec<CatalogPublicationReference>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else {
            return Ok(Vec::new());
        };
        let Some(page) = page_in(&connection, source_id, generation, locator)? else {
            return Ok(Vec::new());
        };
        let mut sequence = connection.prepare_cached(
            "SELECT sequence FROM catalog_publication
             WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
        )?;
        unique_page_publications(&page.page)
            .into_iter()
            .map(|publication| {
                let stored = sequence
                    .query_row(params![source_id.as_str(), storage_i64(generation, "generation")?, publication.identity.edition_id, publication.identity.work_id.as_deref().unwrap_or("")], |row| row.get::<_, i64>(0))
                    .optional()?
                    .ok_or_else(|| CatalogCacheError::new("catalog page references a missing publication"))?;
                Ok(CatalogPublicationReference { identity: publication.identity.clone(), sequence: stored_u64(stored, "publication sequence")? })
            })
            .collect()
    }

    pub fn page_response(&self, source_id: &CatalogSourceId, locator: &str, request_epoch: Option<&str>, known: &[CatalogPublicationReference]) -> Result<Option<CatalogPageResponse>, CatalogCacheError> {
        let connection = self.pool.get()?;
        let Some(state) = source_state_connection(&connection, source_id)? else {
            return Ok(None);
        };
        let Some(mut page) = page_in(&connection, source_id, state.active_generation, locator)? else {
            return Ok(None);
        };
        let known = if request_epoch == Some(state.epoch.as_str()) { known.iter().map(|item| (item.identity.clone(), item.sequence)).collect::<HashMap<_, _>>() } else { HashMap::new() };
        let mut payloads = Vec::new();
        let mut references = HashMap::new();
        let mut popularity = HashMap::new();
        for publication in unique_page_publications(&page.page) {
            let (sequenced, score) =
                sequenced_publication_with_popularity_in(&connection, source_id, state.active_generation, &publication.identity)?.ok_or_else(|| CatalogCacheError::new("catalog page references a missing publication"))?;
            let reference = CatalogPublicationReference { identity: publication.identity.clone(), sequence: sequenced.sequence };
            if known.get(&reference.identity) != Some(&reference.sequence) {
                payloads.push(CatalogPublicationPayload { publication: sequenced.publication, sequence: reference.sequence });
            }
            popularity.insert(reference.identity.clone(), score);
            references.insert(reference.identity.clone(), reference);
        }
        sort_page_publications_by_popularity(&mut page.page, &popularity);
        let layout = page_layout(&page.page, |publication| references[&publication.identity].clone());
        Ok(Some(CatalogPageResponse { epoch: state.epoch, page: layout, payloads }))
    }

    pub fn apply_page_response(&self, source_url: &str, response: CatalogPageResponse) -> Result<CatalogPage, CatalogCacheError> {
        self.set_provider_epoch(&response.page.source_id, source_url, &response.epoch)?;
        let connection = self.pool.get()?;
        let state = source_state_connection(&connection, &response.page.source_id)?.ok_or_else(|| CatalogCacheError::new("catalog source disappeared"))?;
        let supplied = response.payloads.iter().map(|payload| (payload.publication.identity.clone(), payload.clone())).collect::<HashMap<_, _>>();
        let mut resolved = HashMap::new();
        for reference in response.page.publications.iter().chain(response.page.groups.iter().flat_map(|group| group.publications.iter())) {
            if resolved.contains_key(&reference.identity) {
                continue;
            }
            if let Some(payload) = supplied.get(&reference.identity).filter(|payload| payload.sequence == reference.sequence) {
                resolved.insert(reference.identity.clone(), payload.publication.clone());
                continue;
            }
            let cached = sequenced_publication_in(&connection, &response.page.source_id, state.active_generation, &reference.identity)?
                .filter(|publication| publication.sequence == reference.sequence)
                .ok_or_else(|| CatalogCacheError::new("catalog server omitted a required publication payload"))?;
            resolved.insert(reference.identity.clone(), cached.publication);
        }
        drop(connection);
        let page = materialize_page(response.page, |reference| resolved.get(&reference.identity).cloned()).ok_or_else(|| CatalogCacheError::new("catalog page could not be reconstructed"))?;
        self.put_page_with_sequences(&page.source_id, source_url, &page, &response.payloads)?;
        Ok(page)
    }

    pub fn search_response(&self, source_id: &CatalogSourceId, query: &str, request_epoch: Option<&str>, known: &[CatalogPublicationReference]) -> Result<Option<CatalogSearchResponse>, CatalogCacheError> {
        let Some(state) = self.source_state(source_id)? else {
            return Ok(None);
        };
        let matches = self.search(source_id, query)?;
        let known = if request_epoch == Some(state.epoch.as_str()) { known.iter().map(|item| (item.identity.clone(), item.sequence)).collect::<HashMap<_, _>>() } else { HashMap::new() };
        let title_matches = matches.title_matches.iter().map(publication_reference).collect::<Vec<_>>();
        let author_matches = matches.author_matches.iter().map(publication_reference).collect::<Vec<_>>();
        let toc_matches = matches.toc_matches.iter().map(publication_reference).collect::<Vec<_>>();
        let payloads = matches
            .title_matches
            .into_iter()
            .chain(matches.author_matches)
            .chain(matches.toc_matches)
            .filter(|publication| known.get(&publication.publication.identity) != Some(&publication.sequence))
            .map(|publication| CatalogPublicationPayload { publication: publication.publication, sequence: publication.sequence })
            .collect();
        Ok(Some(CatalogSearchResponse { source_id: source_id.clone(), epoch: state.epoch, query: query.to_owned(), title_matches, author_matches, toc_matches, payloads }))
    }

    pub fn apply_search_response(&self, source_url: &str, response: CatalogSearchResponse) -> Result<CatalogSearchMatches, CatalogCacheError> {
        self.set_provider_epoch(&response.source_id, source_url, &response.epoch)?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, &response.source_id, source_url)?;
        let generation = active_generation_in(&transaction, &response.source_id)?;
        for payload in &response.payloads {
            upsert_publication_with_sequence(&transaction, &response.source_id, generation, &payload.publication, payload.sequence)?;
        }
        refresh_active_counts(&transaction, &response.source_id, generation)?;
        let resolve = |reference: &CatalogPublicationReference| -> Result<SequencedCatalogPublication, CatalogCacheError> {
            let publication = transaction
                .query_row(
                    "SELECT payload, sequence FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
                    params![response.source_id.as_str(), storage_i64(generation, "generation")?, reference.identity.edition_id, reference.identity.work_id.as_deref().unwrap_or("")],
                    decode_sequenced_publication,
                )
                .optional()?
                .filter(|publication| publication.sequence == reference.sequence)
                .ok_or_else(|| CatalogCacheError::new("catalog server omitted a required search publication payload"))?;
            Ok(publication)
        };
        let title_matches = response.title_matches.iter().map(&resolve).collect::<Result<Vec<_>, _>>()?;
        let author_matches = response.author_matches.iter().map(&resolve).collect::<Result<Vec<_>, _>>()?;
        let toc_matches = response.toc_matches.iter().map(&resolve).collect::<Result<Vec<_>, _>>()?;
        transaction.commit()?;
        Ok(CatalogSearchMatches { title_matches, author_matches, toc_matches })
    }

    pub fn search(&self, source_id: &CatalogSourceId, query: &str) -> Result<CatalogSearchMatches, CatalogCacheError> {
        let normalized_query = normalize_search_text(query);
        if normalized_query.is_empty() {
            return Ok(CatalogSearchMatches::default());
        }
        let connection = self.pool.get()?;
        let Some(generation) = active_generation_optional(&connection, source_id)? else {
            return Ok(CatalogSearchMatches::default());
        };
        let generation = storage_i64(generation, "generation")?;
        let mut title_matches = Vec::new();
        let mut statement = connection.prepare(
            "SELECT publication.payload, publication.sequence FROM catalog_publication publication
             LEFT JOIN catalog_publication_popularity popularity
               ON popularity.source_id=publication.source_id AND popularity.generation=publication.generation
              AND popularity.edition_id=publication.edition_id AND popularity.work_id=publication.work_id
             WHERE publication.source_id=?1 AND publication.generation=?2 AND instr(publication.normalized_title, ?3)>0
             ORDER BY COALESCE(popularity.reading_log_count,0) DESC, COALESCE(popularity.rating_count,0) DESC,
                      publication.normalized_title, publication.edition_id, publication.work_id",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), generation, normalized_query], decode_sequenced_publication)?;
        for row in rows {
            title_matches.push(row?);
        }

        let mut author_matches = Vec::new();
        let mut statement = connection.prepare(
            "SELECT publication.payload, publication.sequence
             FROM catalog_publication publication
             LEFT JOIN catalog_publication_popularity popularity
               ON popularity.source_id=publication.source_id AND popularity.generation=publication.generation
              AND popularity.edition_id=publication.edition_id AND popularity.work_id=publication.work_id
             WHERE publication.source_id=?1 AND publication.generation=?2
               AND instr(publication.normalized_title, ?3)=0
               AND EXISTS (
                   SELECT 1 FROM catalog_publication_author author
                   WHERE author.source_id=publication.source_id
                     AND author.generation=publication.generation
                     AND author.edition_id=publication.edition_id
                     AND author.work_id=publication.work_id
                     AND instr(author.normalized_name, ?3)>0
               )
             ORDER BY COALESCE(popularity.reading_log_count,0) DESC, COALESCE(popularity.rating_count,0) DESC,
                      publication.normalized_title, publication.edition_id, publication.work_id",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), generation, normalized_query], decode_sequenced_publication)?;
        for row in rows {
            author_matches.push(row?);
        }
        let mut toc_matches = Vec::new();
        let mut statement = connection.prepare(
            "SELECT publication.payload, publication.sequence
             FROM catalog_publication publication
             LEFT JOIN catalog_publication_popularity popularity
               ON popularity.source_id=publication.source_id AND popularity.generation=publication.generation
              AND popularity.edition_id=publication.edition_id AND popularity.work_id=publication.work_id
             WHERE publication.source_id=?1 AND publication.generation=?2
               AND instr(publication.normalized_title, ?3)=0
               AND NOT EXISTS (
                   SELECT 1 FROM catalog_publication_author author
                   WHERE author.source_id=publication.source_id
                     AND author.generation=publication.generation
                     AND author.edition_id=publication.edition_id
                     AND author.work_id=publication.work_id
                     AND instr(author.normalized_name, ?3)>0
               )
               AND EXISTS (
                   SELECT 1 FROM catalog_publication_toc toc
                   WHERE toc.source_id=publication.source_id
                     AND toc.generation=publication.generation
                     AND toc.edition_id=publication.edition_id
                     AND toc.work_id=publication.work_id
                     AND instr(toc.normalized_title, ?3)>0
               )
             ORDER BY COALESCE(popularity.reading_log_count,0) DESC, COALESCE(popularity.rating_count,0) DESC,
                      publication.normalized_title, publication.edition_id, publication.work_id",
        )?;
        let rows = statement.query_map(params![source_id.as_str(), generation, normalized_query], decode_sequenced_publication)?;
        for row in rows {
            toc_matches.push(row?);
        }
        Ok(CatalogSearchMatches { title_matches, author_matches, toc_matches })
    }
    pub(crate) fn put_page_with_sequences(&self, source_id: &CatalogSourceId, source_url: &str, page: &CatalogPage, payloads: &[CatalogPublicationPayload]) -> Result<(), CatalogCacheError> {
        let supplied = payloads.iter().map(|payload| (payload.publication.identity.clone(), payload.sequence)).collect::<HashMap<_, _>>();
        let page_payload = wire::encode(page)?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, source_id, source_url)?;
        let generation = active_generation_in(&transaction, source_id)?;
        put_page_row(&transaction, source_id, generation, page, &page_payload, None, None)?;
        for publication in unique_page_publications(page) {
            let sequence =
                supplied.get(&publication.identity).copied().or_else(|| sequence_in(&transaction, source_id, generation, &publication.identity).ok().flatten()).ok_or_else(|| CatalogCacheError::new("publication sequence is unavailable"))?;
            upsert_publication_with_sequence(&transaction, source_id, generation, publication, sequence)?;
        }
        refresh_active_counts(&transaction, source_id, generation)?;
        transaction.commit()?;
        Ok(())
    }
}

pub(crate) fn put_page_row(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, page: &CatalogPage, payload: &[u8], etag: Option<&str>, last_modified: Option<&str>) -> Result<(), CatalogCacheError> {
    transaction.execute(
        "INSERT INTO catalog_page(source_id, generation, locator, payload, etag, last_modified) VALUES(?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(source_id, generation, locator) DO UPDATE SET payload=excluded.payload, etag=excluded.etag, last_modified=excluded.last_modified",
        params![source_id.as_str(), storage_i64(generation, "generation")?, page.locator, payload, etag, last_modified],
    )?;
    replace_page_links(transaction, source_id.as_str(), storage_i64(generation, "generation")?, &page.locator, page)?;
    Ok(())
}

pub(crate) fn put_prepared_page_rows(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, pages: &[PreparedCatalogPageWrite]) -> Result<(), CatalogCacheError> {
    for prepared_page in pages {
        put_page_row(transaction, source_id, generation, &prepared_page.write.page, &prepared_page.payload, prepared_page.write.etag.as_deref(), prepared_page.write.last_modified.as_deref())?;
        for publication in &prepared_page.publications {
            upsert_active_prepared_publication(transaction, source_id, generation, publication)?;
            queue_publication_extraction(transaction, source_id, generation, &publication.publication, false)?;
        }
    }
    Ok(())
}

pub(crate) fn replace_page_links(transaction: &Transaction<'_>, source_id: &str, generation: i64, locator: &str, page: &CatalogPage) -> Result<(), CatalogCacheError> {
    transaction.execute("DELETE FROM catalog_page_link WHERE source_id=?1 AND generation=?2 AND from_locator=?3", params![source_id, generation, locator])?;
    let mut seen = HashSet::new();
    let navigation = page.navigation.iter().chain(page.groups.iter().flat_map(|group| group.navigation.iter())).filter_map(|link| match &link.target {
        CatalogLinkTarget::Page { locator } => Some(locator.as_str()),
        CatalogLinkTarget::External { .. } => None,
    });
    for target in navigation
        .chain(page.groups.iter().filter_map(|group| group.page_locator.as_deref()))
        .chain([page.pagination.first.as_deref(), page.pagination.previous.as_deref(), page.pagination.next.as_deref(), page.pagination.last.as_deref()].into_iter().flatten())
    {
        if seen.insert(target) {
            transaction.execute("INSERT OR IGNORE INTO catalog_page_link(source_id,generation,from_locator,to_locator) VALUES(?1,?2,?3,?4)", params![source_id, generation, locator, target])?;
        }
    }
    Ok(())
}

pub(crate) fn page_in(connection: &Connection, source_id: &CatalogSourceId, generation: u64, locator: &str) -> Result<Option<CachedCatalogPage>, CatalogCacheError> {
    connection
        .query_row("SELECT payload, etag, last_modified FROM catalog_page WHERE source_id=?1 AND generation=?2 AND locator=?3", params![source_id.as_str(), storage_i64(generation, "generation")?, locator], |row| {
            let payload: Vec<u8> = row.get(0)?;
            let page = decode_page(&payload).map_err(sql_decode_error)?;
            Ok(CachedCatalogPage { page, etag: row.get(1)?, last_modified: row.get(2)? })
        })
        .optional()
        .map_err(Into::into)
}

pub(crate) fn unique_page_publications(page: &CatalogPage) -> Vec<&CatalogPublication> {
    let mut seen = HashSet::new();
    page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).filter(|publication| seen.insert((publication.identity.edition_id.as_str(), publication.identity.work_id.as_deref()))).collect()
}

pub(crate) fn publication_key(publication: &CatalogPublication) -> (&str, &str) {
    (&publication.identity.edition_id, publication.identity.work_id.as_deref().unwrap_or(""))
}

pub(crate) fn allocate_sequence(transaction: &Transaction<'_>, source_id: &CatalogSourceId) -> Result<i64, CatalogCacheError> {
    let next = next_sequence_in(transaction, source_id)?.checked_add(1).ok_or_else(|| CatalogCacheError::new("publication sequence overflow"))?;
    transaction.execute("UPDATE catalog_source SET next_sequence=?2 WHERE source_id=?1", params![source_id.as_str(), storage_i64(next, "publication sequence")?])?;
    storage_i64(next, "publication sequence")
}

pub(crate) fn decode_sequenced_publication(row: &rusqlite::Row<'_>) -> rusqlite::Result<SequencedCatalogPublication> {
    let payload: Vec<u8> = row.get(0)?;
    let publication = decode_cached(&payload).map_err(sql_decode_error)?;
    let sequence = stored_u64(row.get(1)?, "publication sequence")?;
    Ok(SequencedCatalogPublication { publication, sequence })
}

pub(crate) fn sequenced_publication_in(connection: &Connection, source_id: &CatalogSourceId, generation: u64, identity: &catalog_contract::CatalogPublicationIdentity) -> Result<Option<SequencedCatalogPublication>, CatalogCacheError> {
    connection
        .query_row(
            "SELECT payload, sequence FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, identity.edition_id, identity.work_id.as_deref().unwrap_or("")],
            decode_sequenced_publication,
        )
        .optional()
        .map_err(Into::into)
}

pub(crate) fn sequenced_publication_with_popularity_in(
    connection: &Connection, source_id: &CatalogSourceId, generation: u64, identity: &CatalogPublicationIdentity,
) -> Result<Option<(SequencedCatalogPublication, (u64, u64))>, CatalogCacheError> {
    connection
        .query_row(
            "SELECT publication.payload, publication.sequence,
                    COALESCE(popularity.reading_log_count,0), COALESCE(popularity.rating_count,0)
             FROM catalog_publication publication
             LEFT JOIN catalog_publication_popularity popularity
               ON popularity.source_id=publication.source_id AND popularity.generation=publication.generation
              AND popularity.edition_id=publication.edition_id AND popularity.work_id=publication.work_id
             WHERE publication.source_id=?1 AND publication.generation=?2
               AND publication.edition_id=?3 AND publication.work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, identity.edition_id, identity.work_id.as_deref().unwrap_or("")],
            |row| {
                let publication = decode_sequenced_publication(row)?;
                let reading_log_count = stored_u64(row.get(2)?, "reading-log count")?;
                let rating_count = stored_u64(row.get(3)?, "rating count")?;
                Ok((publication, (reading_log_count, rating_count)))
            },
        )
        .optional()
        .map_err(Into::into)
}

pub(crate) fn sort_page_publications_by_popularity(page: &mut CatalogPage, popularity: &HashMap<CatalogPublicationIdentity, (u64, u64)>) {
    let sort = |publications: &mut Vec<CatalogPublication>| {
        // `sort_by` is stable, so equally popular and unenriched books retain
        // the catalogue provider's order.
        publications.sort_by(|left, right| popularity.get(&right.identity).unwrap_or(&(0, 0)).cmp(popularity.get(&left.identity).unwrap_or(&(0, 0))));
    };
    sort(&mut page.publications);
    for group in &mut page.groups {
        sort(&mut group.publications);
    }
}

pub(crate) fn project_gutenberg_download_count(publication: &CatalogPublication) -> Option<u64> {
    if !publication.identity.edition_id.starts_with("urn:gutenberg:") {
        return None;
    }
    let description = match publication.description.as_ref()? {
        CatalogDescription::PlainText { text } => text.as_str(),
        CatalogDescription::RichHtml { plain_text, .. } => plain_text.as_str(),
    };
    let value = description.rsplit_once("Downloads:")?.1.trim_start();
    let digits = value.chars().take_while(char::is_ascii_digit).collect::<String>();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

pub(crate) fn sequence_in(transaction: &Transaction<'_>, source_id: &CatalogSourceId, generation: u64, identity: &catalog_contract::CatalogPublicationIdentity) -> Result<Option<u64>, CatalogCacheError> {
    transaction
        .query_row(
            "SELECT sequence FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, identity.edition_id, identity.work_id.as_deref().unwrap_or("")],
            |row| stored_u64(row.get(0)?, "publication sequence"),
        )
        .optional()
        .map_err(Into::into)
}

pub(crate) fn publication_reference(publication: &SequencedCatalogPublication) -> CatalogPublicationReference {
    CatalogPublicationReference { identity: publication.publication.identity.clone(), sequence: publication.sequence }
}

pub(crate) fn decode_page(payload: &[u8]) -> Result<CatalogPage, wire::WireError> {
    decode_cached(payload)
}

pub(crate) fn decode_cached<T: serde::de::DeserializeOwned>(payload: &[u8]) -> Result<T, wire::WireError> {
    wire::decode(payload, wire::MAX_DECODED_RESPONSE_BYTES)
}

pub(crate) fn sql_decode_error(error: impl std::error::Error + Send + Sync + 'static) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Blob, Box::new(error))
}

pub(crate) fn storage_i64(value: u64, name: &str) -> Result<i64, CatalogCacheError> {
    i64::try_from(value).map_err(|_| CatalogCacheError::new(format!("{name} exceeds SQLite integer range")))
}

pub(crate) fn stored_u64(value: i64, name: &str) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Integer, Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{name} is negative")))))
}

pub(crate) fn optional_stored_u64(value: Option<i64>, name: &str) -> rusqlite::Result<Option<u64>> {
    value.map(|value| stored_u64(value, name)).transpose()
}
