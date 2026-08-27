use catalog_core::{
    active_generation_in, ensure_source_in, put_page_row, refresh_active_counts, sql_decode_error, storage_i64, stored_u64, upsert_publication_with_sequence, CatalogCache, CatalogCacheError, CatalogSearchMatches,
    SequencedCatalogPublication,
};
use catalog_model::normalize_search_text;
use catalog_model::{CatalogPage, CatalogPublication, CatalogSourceId};
use catalog_protocol::{materialize_page, CatalogPageResponse, CatalogPublicationReference, CatalogSearchResponse};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::collections::{HashMap, HashSet};

use crate::client_store::ClientCatalogStore;

/// Client-owned resolution and offline-query behavior over the neutral cache
/// schema. The server never uses these response-materialization paths.
impl ClientCatalogStore {
    pub(super) fn known_page_publications(&self, source_id: &CatalogSourceId, locator: &str) -> Result<Vec<CatalogPublicationReference>, CatalogCacheError> {
        let connection = self.connection.lock().map_err(|_| CatalogCacheError::new("catalogue page cache lock was poisoned"))?;
        let Some(generation) = active_generation(&connection, source_id)? else {
            return Ok(Vec::new());
        };
        let page = connection
            .query_row("SELECT payload FROM catalog_page WHERE source_id=?1 AND generation=?2 AND locator=?3", params![source_id.as_str(), storage_i64(generation, "generation")?, locator], |row| row.get::<_, Vec<u8>>(0))
            .optional()?;
        let Some(page) = page else {
            return Ok(Vec::new());
        };
        let page: CatalogPage = storage_wire::decode(&page, storage_wire::MAX_DECODED_RESPONSE_BYTES).map_err(|error| CatalogCacheError::new(error.to_string()))?;
        let mut sequence = connection.prepare_cached(
            "SELECT sequence FROM catalog_publication
             WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
        )?;
        unique_publications(&page)
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

    pub(super) fn apply_page_response(&self, cache: &CatalogCache, source_url: &str, response: CatalogPageResponse) -> Result<CatalogPage, CatalogCacheError> {
        cache.set_provider_epoch(&response.page.source_id, source_url, &response.epoch)?;
        let mut connection = self.connection.lock().map_err(|_| CatalogCacheError::new("catalogue page cache lock was poisoned"))?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, &response.page.source_id, source_url)?;
        let generation = active_generation_in(&transaction, &response.page.source_id)?;
        let mut supplied = HashMap::new();
        for payload in &response.payloads {
            if supplied.insert(payload.publication.identity.clone(), payload.clone()).is_some() {
                return Err(CatalogCacheError::new("catalog server returned duplicate page publication payloads"));
            }
        }
        let mut resolved = HashMap::new();
        let mut expected_sequences = HashMap::new();
        for reference in response.page.publications.iter().chain(response.page.groups.iter().flat_map(|group| group.publications.iter())) {
            if expected_sequences.insert(reference.identity.clone(), reference.sequence).is_some_and(|sequence| sequence != reference.sequence) {
                return Err(CatalogCacheError::new("catalog page contains conflicting publication sequences"));
            }
            if resolved.contains_key(&reference.identity) {
                continue;
            }
            if let Some(payload) = supplied.get(&reference.identity).filter(|payload| payload.sequence == reference.sequence) {
                resolved.insert(reference.identity.clone(), payload.publication.clone());
                continue;
            }
            let cached = sequenced_publication(&transaction, &response.page.source_id, generation, reference)?.ok_or_else(|| CatalogCacheError::new("catalog server omitted a required publication payload"))?;
            resolved.insert(reference.identity.clone(), cached.publication);
        }
        if supplied.values().any(|payload| expected_sequences.get(&payload.publication.identity) != Some(&payload.sequence)) {
            return Err(CatalogCacheError::new("catalog server returned a payload outside the page layout"));
        }
        let page = materialize_page(response.page, |reference| resolved.get(&reference.identity).cloned()).ok_or_else(|| CatalogCacheError::new("catalog page could not be reconstructed"))?;
        let page_payload = storage_wire::encode(&page).map_err(|error| CatalogCacheError::new(error.to_string()))?;
        put_page_row(&transaction, &page.source_id, generation, &page, &page_payload, None, None, false)?;
        for publication in unique_publications(&page) {
            let sequence = expected_sequences.get(&publication.identity).copied().ok_or_else(|| CatalogCacheError::new("publication sequence is unavailable"))?;
            upsert_publication_with_sequence(&transaction, &page.source_id, generation, publication, sequence)?;
        }
        refresh_active_counts(&transaction, &page.source_id, generation)?;
        transaction.commit()?;
        Ok(page)
    }

    pub(super) fn apply_search_response(&self, cache: &CatalogCache, source_url: &str, response: CatalogSearchResponse) -> Result<CatalogSearchMatches, CatalogCacheError> {
        cache.set_provider_epoch(&response.source_id, source_url, &response.epoch)?;
        let mut connection = self.connection.lock().map_err(|_| CatalogCacheError::new("catalogue search cache lock was poisoned"))?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_source_in(&transaction, &response.source_id, source_url)?;
        let generation = active_generation_in(&transaction, &response.source_id)?;
        let mut expected_sequences = HashMap::new();
        for reference in response.title_matches.iter().chain(&response.author_matches).chain(&response.toc_matches) {
            if expected_sequences.insert(reference.identity.clone(), reference.sequence).is_some_and(|sequence| sequence != reference.sequence) {
                return Err(CatalogCacheError::new("catalog search contains conflicting publication sequences"));
            }
        }
        let mut supplied = HashSet::new();
        for payload in &response.payloads {
            if expected_sequences.get(&payload.publication.identity) != Some(&payload.sequence) {
                return Err(CatalogCacheError::new("catalog server returned a payload outside the search results"));
            }
            if !supplied.insert(payload.publication.identity.clone()) {
                return Err(CatalogCacheError::new("catalog server returned duplicate search publication payloads"));
            }
            upsert_publication_with_sequence(&transaction, &response.source_id, generation, &payload.publication, payload.sequence)?;
        }
        refresh_active_counts(&transaction, &response.source_id, generation)?;
        let resolve = |reference: &CatalogPublicationReference| -> Result<SequencedCatalogPublication, CatalogCacheError> {
            sequenced_publication(&transaction, &response.source_id, generation, reference)?.ok_or_else(|| CatalogCacheError::new("catalog server omitted a required search publication payload"))
        };
        let title_matches = response.title_matches.iter().map(&resolve).collect::<Result<Vec<_>, _>>()?;
        let author_matches = response.author_matches.iter().map(&resolve).collect::<Result<Vec<_>, _>>()?;
        let toc_matches = response.toc_matches.iter().map(&resolve).collect::<Result<Vec<_>, _>>()?;
        transaction.commit()?;
        Ok(CatalogSearchMatches { title_matches, author_matches, toc_matches })
    }

    pub(super) fn search(&self, source_id: &CatalogSourceId, query: &str) -> Result<CatalogSearchMatches, CatalogCacheError> {
        let normalized_query = normalize_search_text(query);
        if normalized_query.is_empty() {
            return Ok(CatalogSearchMatches::default());
        }
        let connection = self.connection.lock().map_err(|_| CatalogCacheError::new("catalogue search cache lock was poisoned"))?;
        let Some(generation) = active_generation(&connection, source_id)? else {
            return Ok(CatalogSearchMatches::default());
        };
        search_snapshot(&connection, source_id, generation, &normalized_query)
    }
}

fn search_snapshot(connection: &Connection, source_id: &CatalogSourceId, generation: u64, normalized_query: &str) -> Result<CatalogSearchMatches, CatalogCacheError> {
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

fn active_generation(connection: &Connection, source_id: &CatalogSourceId) -> Result<Option<u64>, CatalogCacheError> {
    connection.query_row("SELECT active_generation FROM catalog_source WHERE source_id=?1", [source_id.as_str()], |row| stored_u64(row.get(0)?, "active generation")).optional().map_err(Into::into)
}

fn sequenced_publication(connection: &Connection, source_id: &CatalogSourceId, generation: u64, reference: &CatalogPublicationReference) -> Result<Option<SequencedCatalogPublication>, CatalogCacheError> {
    connection
        .query_row(
            "SELECT payload, sequence FROM catalog_publication WHERE source_id=?1 AND generation=?2 AND edition_id=?3 AND work_id=?4",
            params![source_id.as_str(), storage_i64(generation, "generation")?, reference.identity.edition_id, reference.identity.work_id.as_deref().unwrap_or("")],
            decode_sequenced_publication,
        )
        .optional()
        .map(|publication| publication.filter(|publication| publication.sequence == reference.sequence))
        .map_err(Into::into)
}

fn decode_sequenced_publication(row: &rusqlite::Row<'_>) -> rusqlite::Result<SequencedCatalogPublication> {
    let payload: Vec<u8> = row.get(0)?;
    let publication = storage_wire::decode(&payload, storage_wire::MAX_DECODED_RESPONSE_BYTES).map_err(sql_decode_error)?;
    let sequence = stored_u64(row.get(1)?, "publication sequence")?;
    Ok(SequencedCatalogPublication { publication, sequence })
}

fn unique_publications(page: &CatalogPage) -> Vec<&CatalogPublication> {
    let mut seen = HashSet::new();
    page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).filter(|publication| seen.insert((publication.identity.edition_id.as_str(), publication.identity.work_id.as_deref()))).collect()
}
