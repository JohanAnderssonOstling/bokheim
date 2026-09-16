use super::*;
use catalog_protocol::{page_layout, CatalogPageResponse, CatalogPublicationPayload, CatalogPublicationReference, CatalogSearchLayout, CatalogSearchResponse};
use std::collections::HashMap;

impl CatalogStore {
    /// Builds the native page delta from the server's authoritative snapshot.
    pub fn page_response(&self, source_id: &CatalogSourceId, locator: &str, request_epoch: Option<&str>, known: &[CatalogPublicationReference]) -> Result<Option<CatalogPageResponse>, CatalogStoreError> {
        let connection = self.connection()?;
        let Some(state) = source_state_in(&connection, source_id)? else {
            return Ok(None);
        };
        let Some(mut page) = stored_page(&connection, source_id, state.active_generation, locator)? else {
            return Ok(None);
        };
        let known = if request_epoch == Some(state.epoch.as_str()) { known.iter().map(|item| (item.identity.clone(), item.sequence)).collect::<HashMap<_, _>>() } else { HashMap::new() };
        let mut payloads = Vec::new();
        let mut references = HashMap::new();
        let mut popularity = HashMap::new();
        for publication in unique_publications(&page) {
            let (sequenced, score) = sequenced_publication_with_popularity(&connection, source_id, state.active_generation, &publication.identity)?.ok_or_else(|| CatalogStoreError::new("catalog page references a missing publication"))?;
            let reference = CatalogPublicationReference { identity: publication.identity.clone(), sequence: sequenced.sequence };
            if known.get(&reference.identity) != Some(&reference.sequence) {
                payloads.push(CatalogPublicationPayload { publication: sequenced.publication, sequence: reference.sequence });
            }
            popularity.insert(reference.identity.clone(), score);
            references.insert(reference.identity.clone(), reference);
        }
        sort_publications_by_popularity(&mut page, &popularity);
        let page = page_layout(&page, |publication| references[&publication.identity].clone());
        Ok(Some(CatalogPageResponse { epoch: state.epoch, page, payloads }))
    }

    /// Builds the native search delta from the server-owned search query.
    pub fn search_response(&self, source_id: &CatalogSourceId, query: &str, request_epoch: Option<&str>, known: &[CatalogPublicationReference]) -> Result<Option<CatalogSearchResponse>, CatalogStoreError> {
        let connection = self.connection()?;
        let Some(state) = source_state_in(&connection, source_id)? else {
            return Ok(None);
        };
        let normalized_query = normalize_search_text(query);
        let matches = if normalized_query.is_empty() { CatalogSearchMatches::default() } else { search_snapshot(&connection, source_id, state.active_generation, &normalized_query)? };
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
        Ok(Some(CatalogSearchResponse { epoch: state.epoch, search: CatalogSearchLayout { source_id: source_id.clone(), query: query.to_owned(), title_matches, author_matches, toc_matches }, payloads }))
    }
}

pub(super) fn search_snapshot(connection: &Connection, source_id: &CatalogSourceId, generation: u64, normalized_query: &str) -> Result<CatalogSearchMatches, CatalogStoreError> {
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

fn stored_page(connection: &Connection, source_id: &CatalogSourceId, generation: u64, locator: &str) -> Result<Option<CatalogPage>, CatalogStoreError> {
    connection
        .query_row("SELECT payload FROM catalog_page WHERE source_id=?1 AND generation=?2 AND locator=?3", params![source_id.as_str(), storage_i64(generation, "generation")?, locator], |row| {
            let payload: Vec<u8> = row.get(0)?;
            decode_cached(&payload).map_err(sql_decode_error)
        })
        .optional()
        .map_err(Into::into)
}

fn sequenced_publication_with_popularity(connection: &Connection, source_id: &CatalogSourceId, generation: u64, identity: &CatalogPublicationIdentity) -> Result<Option<(SequencedCatalogPublication, (u64, u64))>, CatalogStoreError> {
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

fn decode_sequenced_publication(row: &rusqlite::Row<'_>) -> rusqlite::Result<SequencedCatalogPublication> {
    let payload: Vec<u8> = row.get(0)?;
    let publication = decode_cached(&payload).map_err(sql_decode_error)?;
    let sequence = stored_u64(row.get(1)?, "publication sequence")?;
    Ok(SequencedCatalogPublication { publication, sequence })
}

fn publication_reference(publication: &SequencedCatalogPublication) -> CatalogPublicationReference {
    CatalogPublicationReference { identity: publication.publication.identity.clone(), sequence: publication.sequence }
}

fn unique_publications(page: &CatalogPage) -> Vec<&CatalogPublication> {
    let mut seen = HashSet::new();
    page.publications.iter().chain(page.groups.iter().flat_map(|group| group.publications.iter())).filter(|publication| seen.insert((publication.identity.edition_id.as_str(), publication.identity.work_id.as_deref()))).collect()
}

fn sort_publications_by_popularity(page: &mut CatalogPage, popularity: &HashMap<CatalogPublicationIdentity, (u64, u64)>) {
    let sort = |publications: &mut Vec<CatalogPublication>| {
        publications.sort_by(|left, right| popularity.get(&right.identity).unwrap_or(&(0, 0)).cmp(popularity.get(&left.identity).unwrap_or(&(0, 0))));
    };
    sort(&mut page.publications);
    for group in &mut page.groups {
        sort(&mut group.publications);
    }
}
