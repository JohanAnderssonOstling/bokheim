use super::{
    book_format_storage, catalog_book_format_from_storage, normalize_search_text, params, storage_i64, stored_u64, unix_timestamp, CachedCatalogSubjectBrowseResponse, CachedCatalogSubjectCollectionState, CatalogBookFormat,
    CatalogBookFormatFacetValue, CatalogCache, CatalogCacheError, CatalogLanguageFacetValue, CatalogLink, CatalogPublicationAction, CatalogSourcedPublication, CatalogSubjectBrowseCapabilities, CatalogSubjectBrowseDeltaResponse,
    CatalogSubjectBrowseRequest, CatalogSubjectBrowseResponse, CatalogSubjectSort, HashSet, OptionalExtension, TransactionBehavior,
};
use catalog_contract::CatalogResultRangeResponse;

impl CatalogCache {
    /// Returns the ordered subset of entry IDs whose immutable publication
    /// payloads are already retained locally for this transport epoch.
    pub fn known_subject_publication_ids(&self, epoch: &str, entry_ids: &[u64]) -> Result<Vec<u64>, CatalogCacheError> {
        if entry_ids.len() > 256 {
            return Err(CatalogCacheError::new("too many catalogue entry IDs"));
        }
        let connection = self.pool.get()?;
        let mut available = connection.prepare_cached("SELECT EXISTS(SELECT 1 FROM catalog_subject_publication_cache WHERE epoch=?1 AND entry_id=?2)")?;
        let mut known = Vec::new();
        for entry_id in entry_ids {
            if available.query_row(params![epoch, storage_i64(*entry_id, "catalogue entry ID")?], |row| row.get::<_, bool>(0))? {
                known.push(*entry_id);
            }
        }
        Ok(known)
    }

    /// Retains payloads supplied for a bounded server-ordered range and then
    /// materializes that range from the union of new and previously cached
    /// publications. `None` means the server omitted an entry the client did
    /// not actually have.
    pub fn cache_subject_browse_delta(&self, delta: CatalogSubjectBrowseDeltaResponse) -> Result<Option<CatalogSubjectBrowseResponse>, CatalogCacheError> {
        if delta.range.returned_items as usize != delta.entry_ids.len() {
            return Err(CatalogCacheError::new("catalogue returned an inconsistent subject result range"));
        }
        let entries = delta.entry_ids.iter().copied().collect::<HashSet<_>>();
        if entries.len() != delta.entry_ids.len() {
            return Err(CatalogCacheError::new("catalogue returned duplicate subject entry IDs"));
        }
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for payload in delta.payloads {
            if !entries.contains(&payload.entry_id) {
                return Err(CatalogCacheError::new("catalogue returned a publication outside the requested subject range"));
            }
            let normalized_authors = payload.publication.publication.authors.iter().map(|author| normalize_search_text(&author.name)).collect::<Vec<_>>();
            let formats = payload
                .publication
                .publication
                .actions
                .iter()
                .filter_map(|action| match action {
                    CatalogPublicationAction::Import { request } => Some(book_format_storage(request.book_format)),
                    CatalogPublicationAction::External { .. } => None,
                })
                .collect::<HashSet<_>>();
            transaction.execute(
                "INSERT INTO catalog_subject_publication_cache(epoch,entry_id,source_id,normalized_title,normalized_authors,normalized_search_terms,languages,formats,publication_payload)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
                 ON CONFLICT(epoch,entry_id) DO UPDATE SET source_id=excluded.source_id,normalized_title=excluded.normalized_title,
                    normalized_authors=excluded.normalized_authors,normalized_search_terms=excluded.normalized_search_terms,
                    languages=excluded.languages,formats=excluded.formats,publication_payload=excluded.publication_payload",
                params![
                    delta.epoch,
                    storage_i64(payload.entry_id, "catalogue entry ID")?,
                    payload.publication.source_id.as_str(),
                    normalize_search_text(&payload.publication.publication.title),
                    serde_json::to_string(&normalized_authors)?,
                    serde_json::to_string(&payload.search_terms)?,
                    serde_json::to_string(&payload.publication.publication.languages)?,
                    serde_json::to_string(&formats)?,
                    wire::encode(&payload.publication)?,
                ],
            )?;
        }
        let mut publications = Vec::with_capacity(delta.entry_ids.len());
        let mut complete = true;
        {
            let mut cached = transaction.prepare_cached("SELECT publication_payload FROM catalog_subject_publication_cache WHERE epoch=?1 AND entry_id=?2")?;
            for entry_id in &delta.entry_ids {
                let payload = cached.query_row(params![delta.epoch, storage_i64(*entry_id, "catalogue entry ID")?], |row| row.get::<_, Vec<u8>>(0)).optional()?;
                let Some(payload) = payload else {
                    complete = false;
                    break;
                };
                publications.push(wire::decode::<CatalogSourcedPublication>(&payload, wire::MAX_DECODED_RESPONSE_BYTES)?);
            }
        }
        transaction.commit()?;
        if !complete {
            return Ok(None);
        }
        Ok(Some(CatalogSubjectBrowseResponse { epoch: delta.epoch, navigation: delta.navigation, publications, range: delta.range, capabilities: delta.capabilities }))
    }

    pub fn cached_subject_collection_state(&self, locator: Option<&str>) -> Result<Option<CachedCatalogSubjectCollectionState>, CatalogCacheError> {
        let locator = subject_collection_key(locator)?;
        let connection = self.pool.get()?;
        connection
            .query_row("SELECT epoch,revision,stored_at FROM catalog_subject_collection WHERE locator=?1", [&locator], |row| {
                Ok(CachedCatalogSubjectCollectionState { epoch: row.get(0)?, revision: stored_u64(row.get(1)?, "catalog transport revision")?, checked_at: stored_u64(row.get(2)?, "subject collection check timestamp")? })
            })
            .optional()
            .map_err(Into::into)
    }

    pub fn touch_subject_collection(&self, locator: Option<&str>, epoch: &str, revision: u64) -> Result<bool, CatalogCacheError> {
        let locator = subject_collection_key(locator)?;
        let connection = self.pool.get()?;
        let changed =
            connection.execute("UPDATE catalog_subject_collection SET stored_at=?4 WHERE locator=?1 AND epoch=?2 AND revision=?3", params![locator, epoch, storage_i64(revision, "catalog transport revision")?, unix_timestamp()?])?;
        Ok(changed == 1)
    }

    /// Returns one UI slice from a complete locator-scoped collection. Search,
    /// filters, facets, ordering, and the UI range are deliberately evaluated
    /// in this local SQLite database rather than encoded in the server cache
    /// identity.
    pub fn cached_subject_browse_response(&self, request: &CatalogSubjectBrowseRequest) -> Result<Option<CachedCatalogSubjectBrowseResponse>, CatalogCacheError> {
        let locator = subject_collection_key(request.locator.as_deref())?;
        let connection = self.pool.get()?;
        let Some((epoch, navigation_payload)) =
            connection.query_row("SELECT epoch,navigation_payload FROM catalog_subject_collection WHERE locator=?1", [&locator], |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?))).optional()?
        else {
            return Ok(None);
        };
        let navigation: Vec<CatalogLink> = wire::decode(&navigation_payload, wire::MAX_DECODED_RESPONSE_BYTES)?;
        let normalized_query = normalize_search_text(&request.query);
        let source_ids = serde_json::to_string(&request.source_ids.iter().map(|source| source.as_str()).collect::<Vec<_>>())?;
        let mut languages = request.supported_languages.iter().filter_map(|language| language.split('-').next().map(str::trim).filter(|language| !language.is_empty()).map(str::to_ascii_lowercase)).collect::<Vec<_>>();
        languages.sort_unstable();
        languages.dedup();
        let languages = serde_json::to_string(&languages)?;
        let mut formats = request.book_formats.iter().copied().map(book_format_storage).collect::<Vec<_>>();
        formats.sort_unstable();
        formats.dedup();
        let formats = serde_json::to_string(&formats)?;
        let base = local_subject_scope_sql();
        let total_sql = format!("SELECT COUNT(*) {base}");
        let total = connection.query_row(&total_sql, params![locator, epoch, normalized_query, source_ids, languages, formats], |row| row.get::<_, i64>(0))?;
        let total = usize::try_from(stored_u64(total, "cached subject result count")?).unwrap_or(usize::MAX);
        let limit = usize::try_from(request.range.limit).unwrap_or(100).clamp(1, 100);
        let offset = usize::try_from(request.range.offset).unwrap_or(usize::MAX);
        let ordering = match request.sort {
            CatalogSubjectSort::Popularity => "membership.popularity_position,membership.entry_id",
            CatalogSubjectSort::Title => "publication.normalized_title,publication.source_id,membership.entry_id",
        };
        let sql = format!(
            "SELECT membership.entry_id,publication.publication_payload
             {base}
             ORDER BY {ordering} LIMIT ?7 OFFSET ?8"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement
            .query_map(params![locator, epoch, normalized_query, source_ids, languages, formats, storage_i64(limit as u64, "cached subject result limit")?, storage_i64(offset as u64, "cached subject result offset")?], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?;
        let mut entry_ids = Vec::new();
        let mut publications = Vec::new();
        for row in rows {
            let (entry_id, payload) = row?;
            entry_ids.push(stored_u64(entry_id, "cached subject entry ID")?);
            publications.push(wire::decode::<CatalogSourcedPublication>(&payload, wire::MAX_DECODED_RESPONSE_BYTES)?);
        }
        let capabilities = local_subject_capabilities(&connection, &locator, &epoch, &normalized_query, &source_ids, &languages, &formats, &request.book_formats, &request.supported_languages)?;
        let range = CatalogResultRangeResponse { offset: u32::try_from(offset).unwrap_or(u32::MAX), returned_items: u32::try_from(publications.len()).unwrap_or(u32::MAX), total_items: u32::try_from(total).unwrap_or(u32::MAX) };
        let response = CatalogSubjectBrowseResponse { epoch: epoch.clone(), navigation, publications, range, capabilities };
        Ok(Some(CachedCatalogSubjectBrowseResponse { response, epoch, entry_ids }))
    }

    pub fn known_subject_collection_chunk(&self, request: &CatalogSubjectBrowseRequest) -> Result<(Option<String>, Option<u64>, Vec<u64>), CatalogCacheError> {
        let locator = subject_collection_key(request.locator.as_deref())?;
        let connection = self.pool.get()?;
        let Some((epoch, revision)) = connection.query_row("SELECT epoch,revision FROM catalog_subject_collection WHERE locator=?1", [&locator], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))).optional()? else {
            return Ok((None, None, Vec::new()));
        };
        let limit = request.range.limit.clamp(1, 100);
        let end = u64::from(request.range.offset).saturating_add(u64::from(limit));
        let mut statement = connection.prepare(
            "SELECT entry_id FROM catalog_subject_collection_member
             WHERE locator=?1 AND popularity_position>=?2 AND popularity_position<?3
             ORDER BY popularity_position",
        )?;
        let rows = statement.query_map(params![locator, storage_i64(request.range.offset as u64, "subject collection offset")?, storage_i64(end, "subject collection offset")?], |row| row.get::<_, i64>(0))?;
        let entry_ids = rows.map(|row| row.map_err(CatalogCacheError::from).and_then(|entry_id| stored_u64(entry_id, "catalogue entry ID").map_err(CatalogCacheError::from))).collect::<Result<Vec<_>, CatalogCacheError>>()?;
        Ok((Some(epoch), Some(stored_u64(revision, "catalog transport revision")?), entry_ids))
    }

    /// Stages one transport chunk. The previous complete collection remains
    /// readable until the final chunk atomically replaces its membership.
    pub fn stage_subject_collection_chunk(&self, request: &CatalogSubjectBrowseRequest, delta: CatalogSubjectBrowseDeltaResponse) -> Result<bool, CatalogCacheError> {
        let locator = subject_collection_key(request.locator.as_deref())?;
        if delta.range.offset != request.range.offset || delta.range.returned_items != u32::try_from(delta.entry_ids.len()).unwrap_or(u32::MAX) {
            return Err(CatalogCacheError::new("catalog subject collection chunk does not match its transport range"));
        }
        let exact = delta.exact_entry_ids.iter().copied().collect::<HashSet<_>>();
        if exact.len() != delta.exact_entry_ids.len() || exact.iter().any(|entry_id| !delta.entry_ids.contains(entry_id)) {
            return Err(CatalogCacheError::new("catalog subject collection contains invalid exact memberships"));
        }
        let entries = delta.entry_ids.iter().copied().collect::<HashSet<_>>();
        if entries.len() != delta.entry_ids.len() {
            return Err(CatalogCacheError::new("catalog subject collection contains duplicate entry IDs"));
        }
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if request.range.offset == 0 {
            transaction.execute("DELETE FROM catalog_subject_collection_staging WHERE locator=?1", [&locator])?;
            transaction.execute("DELETE FROM catalog_subject_collection_sync WHERE locator=?1", [&locator])?;
            transaction.execute(
                "INSERT INTO catalog_subject_collection_sync(locator,epoch,revision,navigation_payload,total_items,next_offset) VALUES(?1,?2,?3,?4,?5,0)",
                params![locator, delta.epoch, storage_i64(delta.revision, "catalog transport revision")?, wire::encode(&delta.navigation)?, storage_i64(delta.range.total_items as u64, "subject collection total")?],
            )?;
        }
        let (sync_epoch, sync_revision, total_items, next_offset) = transaction
            .query_row("SELECT epoch,revision,total_items,next_offset FROM catalog_subject_collection_sync WHERE locator=?1", [&locator], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?))
            })
            .optional()?
            .ok_or_else(|| CatalogCacheError::new("catalog subject collection synchronization was not started"))?;
        if sync_epoch != delta.epoch
            || stored_u64(sync_revision, "catalog transport revision")? != delta.revision
            || stored_u64(total_items, "subject collection total")? != u64::from(delta.range.total_items)
            || stored_u64(next_offset, "subject collection offset")? != u64::from(delta.range.offset)
        {
            return Err(CatalogCacheError::new("catalog subject collection changed while it was being synchronized"));
        }
        for payload in delta.payloads {
            if !entries.contains(&payload.entry_id) {
                continue;
            }
            let normalized_authors = payload.publication.publication.authors.iter().map(|author| normalize_search_text(&author.name)).collect::<Vec<_>>();
            let formats = payload
                .publication
                .publication
                .actions
                .iter()
                .filter_map(|action| match action {
                    CatalogPublicationAction::Import { request } => Some(book_format_storage(request.book_format)),
                    CatalogPublicationAction::External { .. } => None,
                })
                .collect::<HashSet<_>>();
            transaction.execute(
                "INSERT INTO catalog_subject_publication_cache(epoch,entry_id,source_id,normalized_title,normalized_authors,normalized_search_terms,languages,formats,publication_payload)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
                 ON CONFLICT(epoch,entry_id) DO UPDATE SET source_id=excluded.source_id,normalized_title=excluded.normalized_title,
                    normalized_authors=excluded.normalized_authors,normalized_search_terms=excluded.normalized_search_terms,
                    languages=excluded.languages,formats=excluded.formats,publication_payload=excluded.publication_payload",
                params![
                    sync_epoch,
                    storage_i64(payload.entry_id, "catalogue entry ID")?,
                    payload.publication.source_id.as_str(),
                    normalize_search_text(&payload.publication.publication.title),
                    serde_json::to_string(&normalized_authors)?,
                    serde_json::to_string(&payload.search_terms)?,
                    serde_json::to_string(&payload.publication.publication.languages)?,
                    serde_json::to_string(&formats)?,
                    wire::encode(&payload.publication)?,
                ],
            )?;
        }
        for (index, entry_id) in delta.entry_ids.iter().copied().enumerate() {
            let available =
                transaction.query_row("SELECT EXISTS(SELECT 1 FROM catalog_subject_publication_cache WHERE epoch=?1 AND entry_id=?2)", params![sync_epoch, storage_i64(entry_id, "catalogue entry ID")?], |row| row.get::<_, bool>(0))?;
            if !available {
                return Err(CatalogCacheError::new("catalog subject collection chunk omitted a required publication payload"));
            }
            let position =
                u64::from(delta.range.offset).checked_add(u64::try_from(index).map_err(|_| CatalogCacheError::new("subject collection position overflow"))?).ok_or_else(|| CatalogCacheError::new("subject collection position overflow"))?;
            transaction.execute(
                "INSERT INTO catalog_subject_collection_staging(locator,entry_id,popularity_position,exact_assignment) VALUES(?1,?2,?3,?4)
                 ON CONFLICT(locator,entry_id) DO UPDATE SET popularity_position=excluded.popularity_position,exact_assignment=excluded.exact_assignment",
                params![locator, storage_i64(entry_id, "catalogue entry ID")?, storage_i64(position, "subject collection position")?, exact.contains(&entry_id)],
            )?;
        }
        let next = u64::from(delta.range.offset) + u64::from(delta.range.returned_items);
        if next < u64::from(delta.range.total_items) && delta.range.returned_items == 0 {
            return Err(CatalogCacheError::new("catalog subject collection synchronization made no progress"));
        }
        transaction.execute("UPDATE catalog_subject_collection_sync SET next_offset=?2 WHERE locator=?1", params![locator, storage_i64(next, "subject collection offset")?])?;
        let complete = next == u64::from(delta.range.total_items);
        if complete {
            let (epoch, revision, navigation_payload) =
                transaction.query_row("SELECT epoch,revision,navigation_payload FROM catalog_subject_collection_sync WHERE locator=?1", [&locator], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, Vec<u8>>(2)?)))?;
            transaction.execute(
                "INSERT INTO catalog_subject_collection(locator,epoch,revision,navigation_payload,stored_at) VALUES(?1,?2,?3,?4,?5)
                 ON CONFLICT(locator) DO UPDATE SET epoch=excluded.epoch,revision=excluded.revision,navigation_payload=excluded.navigation_payload,stored_at=excluded.stored_at",
                params![locator, epoch, revision, navigation_payload, unix_timestamp()?],
            )?;
            transaction.execute("DELETE FROM catalog_subject_collection_member WHERE locator=?1", [&locator])?;
            transaction.execute(
                "INSERT INTO catalog_subject_collection_member(locator,entry_id,popularity_position,exact_assignment)
                 SELECT locator,entry_id,popularity_position,exact_assignment FROM catalog_subject_collection_staging WHERE locator=?1",
                [&locator],
            )?;
            transaction.execute("DELETE FROM catalog_subject_collection_staging WHERE locator=?1", [&locator])?;
            transaction.execute("DELETE FROM catalog_subject_collection_sync WHERE locator=?1", [&locator])?;
            transaction.execute(
                "DELETE FROM catalog_subject_publication_cache
                  WHERE NOT EXISTS(
                        SELECT 1 FROM catalog_subject_collection collection
                        JOIN catalog_subject_collection_member membership ON membership.locator=collection.locator
                       WHERE collection.epoch=catalog_subject_publication_cache.epoch
                         AND membership.entry_id=catalog_subject_publication_cache.entry_id)
                    AND NOT EXISTS(
                        SELECT 1 FROM catalog_subject_collection_sync sync
                        JOIN catalog_subject_collection_staging staging ON staging.locator=sync.locator
                       WHERE sync.epoch=catalog_subject_publication_cache.epoch
                         AND staging.entry_id=catalog_subject_publication_cache.entry_id)",
                [],
            )?;
        }
        transaction.commit()?;
        Ok(complete)
    }
}

fn subject_collection_key(locator: Option<&str>) -> Result<String, CatalogCacheError> {
    locator.map(super::subject_browse::canonical_subject_locator).transpose().map(|locator| locator.unwrap_or_default())
}

fn local_subject_scope_sql() -> &'static str {
    "FROM catalog_subject_collection_member membership
     JOIN catalog_subject_publication_cache publication
       ON publication.epoch=?2 AND publication.entry_id=membership.entry_id
    WHERE membership.locator=?1
      AND (?3!='' OR membership.exact_assignment=1)
      AND (json_array_length(?4)=0 OR publication.source_id IN (SELECT value FROM json_each(?4)))
      AND (json_array_length(?5)=0 OR json_array_length(publication.languages)=0 OR EXISTS(
            SELECT 1 FROM json_each(publication.languages) language
            JOIN json_each(?5) supported
              ON lower(substr(language.value,1,instr(language.value || '-', '-')-1))=
                 lower(substr(supported.value,1,instr(supported.value || '-', '-')-1))))
      AND (json_array_length(?6)=0 OR EXISTS(
            SELECT 1 FROM json_each(publication.formats) format
            JOIN json_each(?6) selected ON selected.value=format.value))
      AND (?3='' OR instr(publication.normalized_title,?3)>0 OR EXISTS(
            SELECT 1 FROM json_each(publication.normalized_authors) author
             WHERE instr(author.value,?3)>0) OR EXISTS(
            SELECT 1 FROM json_each(publication.normalized_search_terms) search_term
             WHERE instr(search_term.value,?3)>0))"
}

#[allow(clippy::too_many_arguments)]
fn local_subject_capabilities(
    connection: &super::Connection, locator: &str, epoch: &str, query: &str, source_ids: &str, languages: &str, formats: &str, selected_formats: &[CatalogBookFormat], selected_languages: &[String],
) -> Result<CatalogSubjectBrowseCapabilities, CatalogCacheError> {
    let exact_and_search = "membership.locator=?1 AND (?3!='' OR membership.exact_assignment=1)
      AND (json_array_length(?4)=0 OR publication.source_id IN (SELECT value FROM json_each(?4)))
      AND (?3='' OR instr(publication.normalized_title,?3)>0 OR EXISTS(
            SELECT 1 FROM json_each(publication.normalized_authors) author WHERE instr(author.value,?3)>0) OR EXISTS(
            SELECT 1 FROM json_each(publication.normalized_search_terms) search_term WHERE instr(search_term.value,?3)>0))";
    let format_sql = format!(
        "SELECT format.value,COUNT(DISTINCT membership.entry_id)
           FROM catalog_subject_collection_member membership
           JOIN catalog_subject_publication_cache publication ON publication.epoch=?2 AND publication.entry_id=membership.entry_id
           JOIN json_each(publication.formats) format
          WHERE {exact_and_search}
            AND (json_array_length(?5)=0 OR json_array_length(publication.languages)=0 OR EXISTS(
                 SELECT 1 FROM json_each(publication.languages) language JOIN json_each(?5) supported
                   ON lower(substr(language.value,1,instr(language.value || '-', '-')-1))=lower(substr(supported.value,1,instr(supported.value || '-', '-')-1))))
            AND length(?6)>=0 GROUP BY format.value"
    );
    let mut statement = connection.prepare(&format_sql)?;
    let mut format_counts = statement
        .query_map(params![locator, epoch, query, source_ids, languages, formats], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?
        .filter_map(|row| match row {
            Ok((format, count)) => catalog_book_format_from_storage(&format).map(|format| Ok(CatalogBookFormatFacetValue { value: format, number_of_items: u32::try_from(count.max(0)).unwrap_or(u32::MAX) })),
            Err(error) => Some(Err(CatalogCacheError::from(error))),
        })
        .collect::<Result<Vec<_>, CatalogCacheError>>()?;
    for selected in selected_formats {
        if !format_counts.iter().any(|facet| facet.value == *selected) {
            format_counts.push(CatalogBookFormatFacetValue { value: *selected, number_of_items: 0 });
        }
    }
    format_counts.sort_by_key(|facet| book_format_storage(facet.value));

    let language_sql = format!(
        "SELECT lower(substr(language.value,1,instr(language.value || '-', '-')-1)),COUNT(DISTINCT membership.entry_id)
           FROM catalog_subject_collection_member membership
           JOIN catalog_subject_publication_cache publication ON publication.epoch=?2 AND publication.entry_id=membership.entry_id
           JOIN json_each(publication.languages) language
          WHERE {exact_and_search}
            AND (json_array_length(?6)=0 OR EXISTS(SELECT 1 FROM json_each(publication.formats) format JOIN json_each(?6) selected ON selected.value=format.value))
            AND length(?5)>=0 GROUP BY 1 ORDER BY 1"
    );
    let mut statement = connection.prepare(&language_sql)?;
    let mut language_counts = statement
        .query_map(params![locator, epoch, query, source_ids, languages, formats], |row| Ok(CatalogLanguageFacetValue { value: row.get(0)?, number_of_items: u32::try_from(row.get::<_, i64>(1)?.max(0)).unwrap_or(u32::MAX) }))?
        .collect::<Result<Vec<_>, _>>()?;
    for selected in selected_languages {
        let value = selected.split('-').next().unwrap_or(selected).to_ascii_lowercase();
        if !value.is_empty() && !language_counts.iter().any(|facet| facet.value == value) {
            language_counts.push(CatalogLanguageFacetValue { value, number_of_items: 0 });
        }
    }
    language_counts.sort_by(|left, right| left.value.cmp(&right.value));
    Ok(CatalogSubjectBrowseCapabilities { sorts: CatalogSubjectSort::ALL.to_vec(), book_formats: format_counts, languages: language_counts })
}
