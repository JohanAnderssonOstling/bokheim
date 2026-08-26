use super::{
    book_format_storage, catalog_book_format_from_storage, decode_cached, normalize_search_text, params, storage_i64, stored_u64, CatalogBookFormatFacetValue, CatalogCache, CatalogCacheError, CatalogLanguageFacetValue, CatalogLink,
    CatalogLinkTarget, CatalogSourceId, CatalogSourcedPublication, CatalogSubjectBrowseCapabilities, CatalogSubjectBrowseCapabilitiesResponse, CatalogSubjectBrowseDeltaResponse, CatalogSubjectBrowseEntryPayload,
    CatalogSubjectBrowseRefreshRequest, CatalogSubjectBrowseRequest, CatalogSubjectBrowseResponse, CatalogSubjectCollectionManifestRequest, CatalogSubjectCollectionManifestResponse, CatalogSubjectPublicationPayloadRequest,
    CatalogSubjectPublicationPayloadResponse, CatalogSubjectSort, Connection, HashSet, TransactionBehavior, CATALOG_SEARCH_INDEX_VERSION, CATALOG_SUBJECT_BROWSE_INDEX_VERSION,
};
use catalog_contract::CatalogResultRangeResponse;
use std::time::Instant;

impl CatalogCache {
    pub fn subject_collection_manifest(&self, request: &CatalogSubjectCollectionManifestRequest) -> Result<CatalogSubjectCollectionManifestResponse, CatalogCacheError> {
        let location = request.locator.as_deref().map(parse_subject_locator).transpose()?.unwrap_or_else(|| SubjectBrowseLocation { system_id: Some(subject_projection::UNIFIED_SYSTEM_ID.to_owned()), subject_path: None });
        let system_id = location.system_id.as_deref().expect("subject system checked above");
        if !browse_subject_system(system_id) {
            return Err(CatalogCacheError::new("unsupported catalogue subject system"));
        }
        let connection = self.pool.get()?;
        connection
            .query_row("SELECT epoch,revision FROM catalog_transport_state WHERE singleton=1", [], |row| Ok(CatalogSubjectCollectionManifestResponse { epoch: row.get(0)?, revision: stored_u64(row.get(1)?, "catalog transport revision")? }))
            .map_err(Into::into)
    }

    /// Browses the same unified taxonomy projection as the local library. The
    /// locator is deliberately opaque to clients so taxonomy storage and
    /// hierarchy rules stay here rather than being duplicated in each UI.
    pub fn browse_subjects(&self, request: &CatalogSubjectBrowseRequest) -> Result<CatalogSubjectBrowseResponse, CatalogCacheError> {
        let page = self.browse_subject_page(request, true, true)?;
        Ok(CatalogSubjectBrowseResponse {
            epoch: page.epoch,
            navigation: page.navigation,
            publications: page.publications.into_iter().filter_map(|publication| publication.item).collect(),
            range: page.range,
            capabilities: page.capabilities,
        })
    }

    pub fn browse_subjects_delta(&self, refresh: &CatalogSubjectBrowseRefreshRequest) -> Result<CatalogSubjectBrowseDeltaResponse, CatalogCacheError> {
        if refresh.known_entry_ids.len() > 256 {
            return Err(CatalogCacheError::new("too many known catalogue entry IDs"));
        }
        let page = if refresh.collection_sync {
            self.browse_subject_collection_page(&refresh.request)?
        } else {
            // Keep the old full-delta behavior for older clients, while making
            // the new ordered-ID phase genuinely free of payload and facet IO.
            self.browse_subject_page(&refresh.request, !refresh.identifiers_only, !refresh.identifiers_only)?
        };
        // Entry IDs include the publication payload hash and are stable within
        // an epoch. A catalogue-wide revision change can alter ordering or
        // membership without invalidating retained payloads for unchanged IDs.
        let known = if refresh.epoch.as_deref() == Some(page.epoch.as_str()) { refresh.known_entry_ids.iter().copied().collect::<HashSet<_>>() } else { HashSet::new() };
        let entry_ids = page.publications.iter().map(|item| item.entry_id).collect();
        let exact_entry_ids = page.publications.iter().filter(|item| item.exact_assignment).map(|item| item.entry_id).collect();
        let payloads = if refresh.identifiers_only {
            Vec::new()
        } else {
            page.publications
                .into_iter()
                .filter(|item| !known.contains(&item.entry_id))
                .filter_map(|item| item.item.map(|publication| CatalogSubjectBrowseEntryPayload { entry_id: item.entry_id, publication, search_terms: item.search_terms }))
                .collect()
        };
        Ok(CatalogSubjectBrowseDeltaResponse { epoch: page.epoch, revision: page.revision, navigation: page.navigation, entry_ids, exact_entry_ids, payloads, range: page.range, capabilities: page.capabilities })
    }

    fn browse_subject_collection_page(&self, request: &CatalogSubjectBrowseRequest) -> Result<SubjectBrowsePage, CatalogCacheError> {
        const MAX_RESULTS: usize = 100;
        let limit = usize::try_from(request.range.limit).unwrap_or(MAX_RESULTS).clamp(1, MAX_RESULTS);
        let offset = usize::try_from(request.range.offset).unwrap_or(usize::MAX);
        let location = request.locator.as_deref().map(parse_subject_locator).transpose()?.unwrap_or_else(|| SubjectBrowseLocation { system_id: Some(subject_projection::UNIFIED_SYSTEM_ID.to_owned()), subject_path: None });
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let (epoch, revision) = transaction.query_row("SELECT epoch,revision FROM catalog_transport_state WHERE singleton=1", [], |row| Ok((row.get(0)?, stored_u64(row.get(1)?, "catalog transport revision")?)))?;
        let system_id = location.system_id.as_deref().expect("subject system checked above");
        let subject_path = location.subject_path.as_deref().unwrap_or("");
        let empty = "[]";
        let navigation_query = SubjectBrowseQuery { system_id, subject_path, normalized_query: "", source_ids: empty, supported_languages: empty, book_formats: empty, sort: CatalogSubjectSort::Popularity, limit, offset };
        let navigation = load_subject_navigation(&transaction, &navigation_query)?;
        let results = load_subject_collection_publications(&transaction, system_id, subject_path, limit, offset)?;
        let range = subject_range(offset, results.publications.len(), results.total);
        transaction.commit()?;
        Ok(SubjectBrowsePage { epoch, revision, navigation, publications: results.publications, range, capabilities: CatalogSubjectBrowseCapabilities::default() })
    }

    pub fn subject_browse_capabilities(&self, request: &CatalogSubjectBrowseRequest) -> Result<CatalogSubjectBrowseCapabilitiesResponse, CatalogCacheError> {
        let prepared = PreparedSubjectBrowse::new(request)?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let (epoch, revision) = catalog_transport_state(&transaction)?;
        let query = prepared.query();
        let started = Instant::now();
        let capabilities = load_subject_capabilities(&transaction, &query, &request.book_formats, &request.supported_languages)?;
        log::info!("catalog_subject_browse phase=capabilities elapsed_ms={}", started.elapsed().as_millis());
        transaction.commit()?;
        Ok(CatalogSubjectBrowseCapabilitiesResponse { epoch, revision, capabilities })
    }

    pub fn subject_publication_payloads(&self, request: &CatalogSubjectPublicationPayloadRequest) -> Result<CatalogSubjectPublicationPayloadResponse, CatalogCacheError> {
        const MAX_ENTRY_IDS: usize = 256;
        if request.entry_ids.len() > MAX_ENTRY_IDS {
            return Err(CatalogCacheError::new("too many catalogue publication entry IDs"));
        }
        let connection = self.pool.get()?;
        let epoch = connection.query_row("SELECT epoch FROM catalog_transport_state WHERE singleton=1", [], |row| row.get::<_, String>(0))?;
        if request.epoch != epoch {
            return Err(CatalogCacheError::new("catalogue publication epoch is stale"));
        }
        let entry_ids = serde_json::to_string(&request.entry_ids)?;
        let started = Instant::now();
        let mut statement = connection.prepare(
            "SELECT p.entry_id,p.source_id,p.payload
               FROM json_each(?1) requested
               JOIN catalog_publication p ON p.entry_id=requested.value
               JOIN catalog_source active ON active.source_id=p.source_id AND active.active_generation=p.generation
              WHERE p.importable=1
              ORDER BY requested.key",
        )?;
        let rows = statement.query_map([entry_ids], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Vec<u8>>(2)?)))?;
        let mut payloads = Vec::with_capacity(request.entry_ids.len());
        for row in rows {
            let (entry_id, source_id, payload) = row?;
            payloads.push(CatalogSubjectBrowseEntryPayload {
                entry_id: stored_u64(entry_id, "subject publication entry ID")?,
                publication: CatalogSourcedPublication { source_id: CatalogSourceId::parse(&source_id).map_err(CatalogCacheError::new)?, publication: decode_cached(&payload)? },
                search_terms: Vec::new(),
            });
        }
        log::info!("catalog_subject_browse phase=payloads requested={} returned={} elapsed_ms={}", request.entry_ids.len(), payloads.len(), started.elapsed().as_millis());
        Ok(CatalogSubjectPublicationPayloadResponse { epoch, payloads })
    }

    fn browse_subject_page(&self, request: &CatalogSubjectBrowseRequest, include_payloads: bool, include_capabilities: bool) -> Result<SubjectBrowsePage, CatalogCacheError> {
        let prepared = PreparedSubjectBrowse::new(request)?;
        let mut connection = self.pool.get()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Deferred)?;
        let (epoch, revision) = catalog_transport_state(&transaction)?;
        let system_id = prepared.location.system_id.as_deref().expect("subject system checked above");
        if !browse_subject_system(system_id) {
            return Err(CatalogCacheError::new("unsupported catalogue subject system"));
        }
        let query = prepared.query();
        let capabilities = if include_capabilities {
            let started = Instant::now();
            let capabilities = load_subject_capabilities(&transaction, &query, &request.book_formats, &request.supported_languages)?;
            log::info!("catalog_subject_browse phase=capabilities elapsed_ms={}", started.elapsed().as_millis());
            capabilities
        } else {
            CatalogSubjectBrowseCapabilities::default()
        };
        let started = Instant::now();
        let navigation = load_subject_navigation(&transaction, &query)?;
        log::info!("catalog_subject_browse phase=navigation elapsed_ms={}", started.elapsed().as_millis());
        let results = load_subject_publications(&transaction, &query, include_payloads)?;
        let range = subject_range(prepared.offset, results.publications.len(), results.total);
        transaction.commit()?;
        Ok(SubjectBrowsePage { epoch, revision, navigation, publications: results.publications, range, capabilities })
    }
}

struct SubjectBrowsePage {
    epoch: String,
    revision: u64,
    navigation: Vec<CatalogLink>,
    publications: Vec<SequencedSubjectPublication>,
    range: CatalogResultRangeResponse,
    capabilities: CatalogSubjectBrowseCapabilities,
}

const SUBJECT_LANGUAGE_FILTER: &str = "(json_array_length(?5)=0 OR NOT EXISTS(
    SELECT 1 FROM catalog_publication_language any_language
     WHERE any_language.source_id=p.source_id AND any_language.generation=p.generation
       AND any_language.edition_id=p.edition_id AND any_language.work_id=p.work_id
 ) OR EXISTS(
    SELECT 1 FROM catalog_publication_language language
    JOIN json_each(?5) supported
      ON lower(substr(language.language,1,instr(language.language || '-', '-')-1))=
         lower(substr(supported.value,1,instr(supported.value || '-', '-')-1))
   WHERE language.source_id=p.source_id AND language.generation=p.generation
     AND language.edition_id=p.edition_id AND language.work_id=p.work_id
 ))";
const SUBJECT_SOURCE_FILTER: &str = "(json_array_length(?4)=0 OR p.source_id IN (SELECT value FROM json_each(?4)))";
const SUBJECT_CANONICAL_EDITION_FILTER: &str = "(
    p.source_id!='opds-project-gutenberg'
    OR p.work_id NOT LIKE 'urn:gutenberg:%'
    OR NOT EXISTS(
        SELECT 1 FROM catalog_publication preferred
         WHERE preferred.source_id=p.source_id AND preferred.generation=p.generation
           AND preferred.work_id=p.work_id AND preferred.importable=1
           AND (preferred.gutenberg_has_images>p.gutenberg_has_images
                OR (preferred.gutenberg_has_images=p.gutenberg_has_images
                    AND preferred.edition_id<p.edition_id))
    )
)";
const SUBJECT_FORMAT_FILTER: &str = "(json_array_length(?6)=0 OR EXISTS(
    SELECT 1 FROM catalog_publication_format format
    JOIN json_each(?6) selected ON selected.value=format.book_format
   WHERE format.source_id=p.source_id AND format.generation=p.generation
     AND format.edition_id=p.edition_id AND format.work_id=p.work_id
))";
const SERVING_SOURCE_FILTER: &str = "(json_array_length(?4)=0 OR browse.source_id IN (SELECT value FROM json_each(?4)))";
const SERVING_LANGUAGE_FILTER: &str = "(json_array_length(?5)=0 OR browse.has_language=0 OR EXISTS(
    SELECT 1 FROM catalog_browse_language language
    JOIN json_each(?5) supported
      ON language.language=lower(substr(supported.value,1,instr(supported.value || '-', '-')-1))
   WHERE language.entry_id=browse.entry_id
))";
const SERVING_FORMAT_FILTER: &str = "(json_array_length(?6)=0 OR EXISTS(
    SELECT 1 FROM catalog_browse_format format
    JOIN json_each(?6) selected ON selected.value=format.book_format
   WHERE format.entry_id=browse.entry_id
))";
const SERVING_SUBJECT_FILTER: &str = "EXISTS(
    SELECT 1 FROM catalog_browse_subject subject
     WHERE subject.entry_id=browse.entry_id AND subject.system_id=?1
       AND (?2='' OR subject.node_path=?2)
)";

struct SubjectPublicationPage {
    publications: Vec<SequencedSubjectPublication>,
    total: usize,
}

struct SequencedSubjectPublication {
    item: Option<CatalogSourcedPublication>,
    entry_id: u64,
    exact_assignment: bool,
    search_terms: Vec<String>,
}

struct SubjectBrowseQuery<'a> {
    system_id: &'a str,
    subject_path: &'a str,
    normalized_query: &'a str,
    source_ids: &'a str,
    supported_languages: &'a str,
    book_formats: &'a str,
    sort: CatalogSubjectSort,
    limit: usize,
    offset: usize,
}

struct PreparedSubjectBrowse {
    location: SubjectBrowseLocation,
    normalized_query: String,
    source_ids: String,
    supported_languages: String,
    book_formats: String,
    sort: CatalogSubjectSort,
    limit: usize,
    offset: usize,
}

impl PreparedSubjectBrowse {
    fn new(request: &CatalogSubjectBrowseRequest) -> Result<Self, CatalogCacheError> {
        const MAX_RESULTS: usize = 100;
        const MAX_SOURCE_FILTERS: usize = 256;
        const MAX_FORMAT_FILTERS: usize = 7;
        if request.source_ids.len() > MAX_SOURCE_FILTERS {
            return Err(CatalogCacheError::new("too many catalogue source filters"));
        }
        if request.book_formats.len() > MAX_FORMAT_FILTERS {
            return Err(CatalogCacheError::new("too many catalogue format filters"));
        }
        let location = request.locator.as_deref().map(parse_subject_locator).transpose()?.unwrap_or_else(|| SubjectBrowseLocation { system_id: Some(subject_projection::UNIFIED_SYSTEM_ID.to_owned()), subject_path: None });
        let system_id = location.system_id.as_deref().expect("subject system checked above");
        if !browse_subject_system(system_id) {
            return Err(CatalogCacheError::new("unsupported catalogue subject system"));
        }
        let mut book_formats = request.book_formats.iter().copied().map(book_format_storage).collect::<Vec<_>>();
        book_formats.sort_unstable();
        book_formats.dedup();
        Ok(Self {
            location,
            normalized_query: normalize_search_text(&request.query),
            source_ids: serde_json::to_string(&request.source_ids.iter().map(CatalogSourceId::as_str).collect::<Vec<_>>())?,
            supported_languages: serde_json::to_string(&request.supported_languages)?,
            book_formats: serde_json::to_string(&book_formats)?,
            sort: request.sort,
            limit: usize::try_from(request.range.limit).unwrap_or(MAX_RESULTS).clamp(1, MAX_RESULTS),
            offset: usize::try_from(request.range.offset).unwrap_or(usize::MAX),
        })
    }

    fn query(&self) -> SubjectBrowseQuery<'_> {
        SubjectBrowseQuery {
            system_id: self.location.system_id.as_deref().expect("subject system checked above"),
            subject_path: self.location.subject_path.as_deref().unwrap_or(""),
            normalized_query: &self.normalized_query,
            source_ids: &self.source_ids,
            supported_languages: &self.supported_languages,
            book_formats: &self.book_formats,
            sort: self.sort,
            limit: self.limit,
            offset: self.offset,
        }
    }
}

fn catalog_transport_state(connection: &Connection) -> Result<(String, u64), CatalogCacheError> {
    connection.query_row("SELECT epoch,revision FROM catalog_transport_state WHERE singleton=1", [], |row| Ok((row.get(0)?, stored_u64(row.get(1)?, "catalog transport revision")?))).map_err(Into::into)
}

fn load_subject_navigation(connection: &Connection, query: &SubjectBrowseQuery<'_>) -> Result<Vec<CatalogLink>, CatalogCacheError> {
    let source_filter = SUBJECT_SOURCE_FILTER;
    let language_filter = SUBJECT_LANGUAGE_FILTER;
    let format_filter = SUBJECT_FORMAT_FILTER;
    let mut navigation = Vec::new();
    if query.normalized_query.is_empty() {
        let materialized = subject_browse_index_ready(connection)?;
        let sql = if materialized {
            format!(
                "SELECT node.title,node.node_path,COUNT(*)
                   FROM catalog_subject_node node
                   JOIN catalog_publication_subject_node membership
                     ON membership.system_id=node.system_id AND membership.node_path=node.node_path
                   JOIN catalog_source s ON s.source_id=membership.source_id AND s.active_generation=membership.generation
                   JOIN catalog_publication p ON p.source_id=membership.source_id AND p.generation=membership.generation
                    AND p.edition_id=membership.edition_id AND p.work_id=membership.work_id
                  WHERE node.system_id=?1 AND node.parent_path=?2 AND p.importable=1
                    AND {SUBJECT_CANONICAL_EDITION_FILTER}
                    AND {source_filter} AND {language_filter} AND {format_filter}
                  GROUP BY node.system_id,node.node_path,node.title
                  ORDER BY node.title"
            )
        } else {
            format!(
                "WITH scoped AS (
                    SELECT CASE WHEN ?2='' THEN assignment.subject_path ELSE substr(assignment.subject_path,length(?2)+4) END AS remainder,
                           p.source_id || char(31) || p.edition_id || char(31) || p.work_id AS publication_key
                      FROM catalog_publication_subject_assignment assignment
                      JOIN catalog_source s ON s.source_id=assignment.source_id AND s.active_generation=assignment.generation
                      JOIN catalog_publication p ON p.source_id=assignment.source_id AND p.generation=assignment.generation
                       AND p.edition_id=assignment.edition_id AND p.work_id=assignment.work_id
                     WHERE assignment.system_id=?1 AND p.importable=1
                       AND (?2='' OR instr(assignment.subject_path,?2 || ' / ')=1)
                       AND {SUBJECT_CANONICAL_EDITION_FILTER}
                       AND {source_filter} AND {language_filter} AND {format_filter}
                 ), children AS (
                    SELECT CASE WHEN instr(remainder,' / ')>0 THEN substr(remainder,1,instr(remainder,' / ')-1) ELSE remainder END AS title,
                           publication_key FROM scoped WHERE remainder!=''
                 )
                 SELECT title,CASE WHEN ?2='' THEN title ELSE ?2 || ' / ' || title END,COUNT(DISTINCT publication_key)
                   FROM children WHERE title!='' GROUP BY title ORDER BY title"
            )
        };
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params![query.system_id, query.subject_path, query.normalized_query, query.source_ids, query.supported_languages, query.book_formats], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
        })?;
        for row in rows {
            let (title, child_path, count) = row?;
            navigation.push(CatalogLink {
                title,
                target: CatalogLinkTarget::Page { locator: subject_locator(&SubjectBrowseLocation { system_id: Some(query.system_id.to_owned()), subject_path: Some(child_path) }) },
                relations: Vec::new(),
                number_of_items: Some(u32::try_from(stored_u64(count, "subject publication count")?).unwrap_or(u32::MAX)),
                active: false,
            });
        }
    }

    Ok(navigation)
}

fn subject_browse_index_ready(connection: &Connection) -> Result<bool, CatalogCacheError> {
    connection.query_row("SELECT EXISTS(SELECT 1 FROM catalog_subject_browse_index_metadata WHERE singleton=1 AND index_version=?1)", [CATALOG_SUBJECT_BROWSE_INDEX_VERSION], |row| row.get(0)).map_err(Into::into)
}

struct SubjectPublicationQuery {
    sql: String,
    search_parameter: String,
}

struct SubjectPublicationCountQuery {
    sql: String,
    search_parameter: String,
}

struct ServingSubjectPublicationQuery {
    sql: String,
    search_parameter: String,
}

struct SubjectScopeQuery {
    matching_publications: String,
    from_clause: String,
    subject_filter: String,
    search_parameter: String,
}

fn build_subject_scope_query(connection: &Connection, normalized_query: &str, subject_path: &str) -> Result<SubjectScopeQuery, CatalogCacheError> {
    let source_filter = SUBJECT_SOURCE_FILTER;
    let query_length = normalized_query.chars().count();
    let search_index_ready = connection.query_row("SELECT index_version=?1 AND ready=1 FROM catalog_search_index_metadata WHERE singleton=1", [CATALOG_SEARCH_INDEX_VERSION], |row| row.get::<_, bool>(0))?;
    if query_length >= 3 && !search_index_ready {
        return Err(CatalogCacheError::new("catalogue search index is warming"));
    }
    if !normalized_query.is_empty() && query_length < 3 {
        return Err(CatalogCacheError::new("catalogue search requires at least three characters"));
    }
    let use_search_index = query_length >= 3 && search_index_ready;
    // Quoting makes the normalized text a literal FTS phrase instead of
    // exposing MATCH operators supplied by an HTTP client. The trigram
    // tokenizer preserves the previous substring-search semantics.
    let search_parameter = if use_search_index { format!("\"{}\"", normalized_query.replace('"', "\"\"")) } else { normalized_query.to_owned() };
    let materialized = subject_browse_index_ready(connection)?;
    let matching_publications = if normalized_query.is_empty() {
        String::new()
    } else if use_search_index {
        format!(
            "WITH matching_publications(source_id,generation,edition_id,work_id) AS MATERIALIZED (
                SELECT p.source_id,p.generation,p.edition_id,p.work_id
                  FROM catalog_publication_title_fts
                  JOIN catalog_publication p ON p.rowid=catalog_publication_title_fts.rowid
                  JOIN catalog_source active ON active.source_id=p.source_id AND active.active_generation=p.generation
                 WHERE p.importable=1 AND {source_filter} AND catalog_publication_title_fts MATCH ?3
                UNION
                SELECT author.source_id,author.generation,author.edition_id,author.work_id
                  FROM catalog_publication_author_fts
                  JOIN catalog_publication_author author ON author.rowid=catalog_publication_author_fts.rowid
                  JOIN catalog_source active ON active.source_id=author.source_id AND active.active_generation=author.generation
                  JOIN catalog_publication p ON p.source_id=author.source_id AND p.generation=author.generation
                   AND p.edition_id=author.edition_id AND p.work_id=author.work_id
                 WHERE p.importable=1 AND {source_filter} AND catalog_publication_author_fts MATCH ?3
                UNION
                SELECT toc.source_id,toc.generation,toc.edition_id,toc.work_id
                  FROM catalog_publication_toc_fts
                  JOIN catalog_publication_toc toc ON toc.rowid=catalog_publication_toc_fts.rowid
                  JOIN catalog_source active ON active.source_id=toc.source_id AND active.active_generation=toc.generation
                  JOIN catalog_publication p ON p.source_id=toc.source_id AND p.generation=toc.generation
                   AND p.edition_id=toc.edition_id AND p.work_id=toc.work_id
                 WHERE p.importable=1 AND {source_filter} AND catalog_publication_toc_fts MATCH ?3
            )"
        )
    } else {
        format!(
            "WITH matching_publications(source_id,generation,edition_id,work_id) AS MATERIALIZED (
                SELECT p.source_id,p.generation,p.edition_id,p.work_id
                  FROM catalog_publication p
                  JOIN catalog_source active ON active.source_id=p.source_id AND active.active_generation=p.generation
                 WHERE p.importable=1 AND {source_filter} AND instr(p.normalized_title,?3)>0
                UNION
                SELECT author.source_id,author.generation,author.edition_id,author.work_id
                  FROM catalog_publication_author author
                  JOIN catalog_source active ON active.source_id=author.source_id AND active.active_generation=author.generation
                  JOIN catalog_publication p ON p.source_id=author.source_id AND p.generation=author.generation
                   AND p.edition_id=author.edition_id AND p.work_id=author.work_id
                 WHERE p.importable=1 AND {source_filter} AND instr(author.normalized_name,?3)>0
                UNION
                SELECT toc.source_id,toc.generation,toc.edition_id,toc.work_id
                  FROM catalog_publication_toc toc
                  JOIN catalog_source active ON active.source_id=toc.source_id AND active.active_generation=toc.generation
                  JOIN catalog_publication p ON p.source_id=toc.source_id AND p.generation=toc.generation
                   AND p.edition_id=toc.edition_id AND p.work_id=toc.work_id
                 WHERE p.importable=1 AND {source_filter} AND instr(toc.normalized_title,?3)>0
            )"
        )
    };
    let (from_clause, subject_filter) = if materialized && normalized_query.is_empty() {
        (
            "FROM catalog_publication_subject_node subject
             JOIN catalog_publication p
               ON p.source_id=subject.source_id AND p.generation=subject.generation
              AND p.edition_id=subject.edition_id AND p.work_id=subject.work_id"
                .to_owned(),
            "subject.system_id=?1 AND subject.node_path=?2 AND subject.exact_assignment=1".to_owned(),
        )
    } else if materialized {
        let node_filter = if subject_path.is_empty() { String::new() } else { " AND subject.node_path=?2".to_owned() };
        (
            "FROM matching_publications matching
             JOIN catalog_publication p
               ON p.source_id=matching.source_id AND p.generation=matching.generation
              AND p.edition_id=matching.edition_id AND p.work_id=matching.work_id"
                .to_owned(),
            format!(
                "EXISTS(SELECT 1 FROM catalog_publication_subject_node subject
                         WHERE subject.source_id=p.source_id AND subject.generation=p.generation
                           AND subject.edition_id=p.edition_id AND subject.work_id=p.work_id
                           AND subject.system_id=?1{node_filter})"
            ),
        )
    } else if normalized_query.is_empty() {
        (
            "FROM catalog_publication p".to_owned(),
            "EXISTS(SELECT 1 FROM catalog_publication_subject_assignment subject
                     WHERE subject.source_id=p.source_id AND subject.generation=p.generation
                       AND subject.edition_id=p.edition_id AND subject.work_id=p.work_id
                       AND subject.system_id=?1 AND subject.subject_path=?2)"
                .to_owned(),
        )
    } else {
        let path_filter = if subject_path.is_empty() { String::new() } else { " AND (subject.subject_path=?2 OR instr(subject.subject_path,?2 || ' / ')=1)".to_owned() };
        (
            "FROM matching_publications matching
             JOIN catalog_publication p
               ON p.source_id=matching.source_id AND p.generation=matching.generation
              AND p.edition_id=matching.edition_id AND p.work_id=matching.work_id"
                .to_owned(),
            format!(
                "EXISTS(SELECT 1 FROM catalog_publication_subject_assignment subject
                         WHERE subject.source_id=p.source_id AND subject.generation=p.generation
                           AND subject.edition_id=p.edition_id AND subject.work_id=p.work_id
                           AND subject.system_id=?1{path_filter})"
            ),
        )
    };
    Ok(SubjectScopeQuery { matching_publications, from_clause, subject_filter, search_parameter })
}

fn browse_serving_index_ready(connection: &Connection) -> Result<bool, CatalogCacheError> {
    connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM catalog_browse_serving_metadata metadata
                WHERE metadata.singleton=1 AND metadata.index_version=1
                  AND metadata.ready=1)",
            [],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn serving_search_parameter(normalized_query: &str) -> String {
    format!("\"{}\"", normalized_query.replace('"', "\"\""))
}

fn build_serving_subject_publication_query(normalized_query: &str, sort: CatalogSubjectSort, include_payloads: bool) -> ServingSubjectPublicationQuery {
    let ordering = match sort {
        CatalogSubjectSort::Popularity => {
            "(page.reading_log_count>0 OR page.rating_count>0) DESC,
             page.reading_log_count DESC,page.rating_count DESC,
             page.provider_popularity DESC,page.sequence DESC,
             page.normalized_title,page.source_id,page.edition_id,page.work_id"
        }
        CatalogSubjectSort::Title => "page.normalized_title,page.source_id,page.edition_id,page.work_id",
    };
    let selection = if include_payloads { "publication.source_id,publication.payload,page.entry_id,page.total" } else { "page.entry_id,page.total" };
    let payload_join = if include_payloads {
        "JOIN catalog_publication publication
           ON publication.source_id=page.source_id AND publication.generation=page.generation
          AND publication.edition_id=page.edition_id AND publication.work_id=page.work_id"
    } else {
        ""
    };
    let sql = format!(
        "WITH matching(entry_id) AS MATERIALIZED (
             SELECT rowid FROM catalog_browse_search_fts
              WHERE catalog_browse_search_fts MATCH ?3
         ), filtered AS MATERIALIZED (
             SELECT browse.*
               FROM matching
               JOIN catalog_browse_publication browse ON browse.entry_id=matching.entry_id
              WHERE {SERVING_SOURCE_FILTER} AND {SERVING_LANGUAGE_FILTER}
                AND {SERVING_FORMAT_FILTER} AND {SERVING_SUBJECT_FILTER}
         ), page AS MATERIALIZED (
             SELECT filtered.*,COUNT(*) OVER() AS total
               FROM filtered
              ORDER BY {ordering_filtered}
              LIMIT ?7 OFFSET ?8
         )
         SELECT {selection}
           FROM page
           {payload_join}
          ORDER BY {ordering}",
        ordering_filtered = ordering.replace("page.", "filtered."),
    );
    ServingSubjectPublicationQuery { sql, search_parameter: serving_search_parameter(normalized_query) }
}

fn build_serving_subject_count_query(normalized_query: &str) -> ServingSubjectPublicationQuery {
    let sql = format!(
        "WITH matching(entry_id) AS MATERIALIZED (
             SELECT rowid FROM catalog_browse_search_fts
              WHERE catalog_browse_search_fts MATCH ?3
         )
         SELECT COUNT(*)
           FROM matching
           JOIN catalog_browse_publication browse ON browse.entry_id=matching.entry_id
          WHERE {SERVING_SOURCE_FILTER} AND {SERVING_LANGUAGE_FILTER}
            AND {SERVING_FORMAT_FILTER} AND {SERVING_SUBJECT_FILTER}"
    );
    ServingSubjectPublicationQuery { sql, search_parameter: serving_search_parameter(normalized_query) }
}

fn build_serving_subject_capabilities_query(normalized_query: &str) -> ServingSubjectPublicationQuery {
    let sql = format!(
        "WITH matching(entry_id) AS MATERIALIZED (
             SELECT rowid FROM catalog_browse_search_fts
              WHERE catalog_browse_search_fts MATCH ?3
         )
         SELECT 'format',format.book_format,COUNT(*)
           FROM matching
           JOIN catalog_browse_publication browse ON browse.entry_id=matching.entry_id
           JOIN catalog_browse_format format ON format.entry_id=browse.entry_id
          WHERE {SERVING_SOURCE_FILTER} AND {SERVING_LANGUAGE_FILTER}
            AND {SERVING_SUBJECT_FILTER}
          GROUP BY format.book_format
         UNION ALL
         SELECT 'language',language.language,COUNT(*)
           FROM matching
           JOIN catalog_browse_publication browse ON browse.entry_id=matching.entry_id
           JOIN catalog_browse_language language ON language.entry_id=browse.entry_id
          WHERE {SERVING_SOURCE_FILTER} AND {SERVING_FORMAT_FILTER}
            AND {SERVING_SUBJECT_FILTER}
          GROUP BY language.language"
    );
    ServingSubjectPublicationQuery { sql, search_parameter: serving_search_parameter(normalized_query) }
}

fn load_subject_capabilities(connection: &Connection, query: &SubjectBrowseQuery<'_>, selected_formats: &[super::CatalogBookFormat], selected_languages: &[String]) -> Result<CatalogSubjectBrowseCapabilities, CatalogCacheError> {
    if !query.normalized_query.is_empty() && browse_serving_index_ready(connection)? {
        return load_serving_subject_capabilities(connection, query, selected_formats, selected_languages);
    }
    let (format_sql, search_parameter) = build_subject_format_capability_query(connection, query.normalized_query, query.subject_path)?;
    let mut statement = connection.prepare(&format_sql)?;
    let rows = statement.query_map(params![query.system_id, query.subject_path, search_parameter, query.source_ids, query.supported_languages, query.book_formats], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
    let mut format_counts = Vec::new();
    for row in rows {
        let (format, count) = row?;
        if let Some(format) = catalog_book_format_from_storage(&format) {
            format_counts.push((format, u32::try_from(stored_u64(count, "catalogue format facet count")?).unwrap_or(u32::MAX)));
        }
    }
    let book_formats = super::CatalogBookFormat::ALL
        .into_iter()
        .filter_map(|value| {
            format_counts
                .iter()
                .find(|(format, _)| *format == value)
                .map(|(_, count)| CatalogBookFormatFacetValue { value, number_of_items: *count })
                .or_else(|| selected_formats.contains(&value).then_some(CatalogBookFormatFacetValue { value, number_of_items: 0 }))
        })
        .collect();

    let (language_sql, search_parameter) = build_subject_language_capability_query(connection, query.normalized_query, query.subject_path)?;
    let mut statement = connection.prepare(&language_sql)?;
    let rows = statement.query_map(params![query.system_id, query.subject_path, search_parameter, query.source_ids, query.supported_languages, query.book_formats], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
    let mut languages = rows
        .map(|row| {
            let (value, count) = row?;
            Ok(CatalogLanguageFacetValue { value, number_of_items: u32::try_from(stored_u64(count, "catalogue language facet count")?).unwrap_or(u32::MAX) })
        })
        .collect::<Result<Vec<_>, CatalogCacheError>>()?;
    for selected in selected_languages {
        let value = selected.split('-').next().unwrap_or(selected).to_ascii_lowercase();
        if !value.is_empty() && !languages.iter().any(|language| language.value == value) {
            languages.push(CatalogLanguageFacetValue { value, number_of_items: 0 });
        }
    }
    languages.sort_by(|left, right| left.value.cmp(&right.value));

    Ok(CatalogSubjectBrowseCapabilities { sorts: CatalogSubjectSort::ALL.to_vec(), book_formats, languages })
}

fn load_serving_subject_capabilities(connection: &Connection, query: &SubjectBrowseQuery<'_>, selected_formats: &[super::CatalogBookFormat], selected_languages: &[String]) -> Result<CatalogSubjectBrowseCapabilities, CatalogCacheError> {
    let capability_query = build_serving_subject_capabilities_query(query.normalized_query);
    let mut statement = connection.prepare(&capability_query.sql)?;
    let rows = statement.query_map(params![query.system_id, query.subject_path, capability_query.search_parameter, query.source_ids, query.supported_languages, query.book_formats], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
    })?;
    let mut format_counts = Vec::new();
    let mut languages = Vec::new();
    for row in rows {
        let (kind, value, count) = row?;
        let count = u32::try_from(stored_u64(count, "catalogue serving facet count")?).unwrap_or(u32::MAX);
        match kind.as_str() {
            "format" => {
                if let Some(format) = catalog_book_format_from_storage(&value) {
                    format_counts.push((format, count));
                }
            }
            "language" => languages.push(CatalogLanguageFacetValue { value, number_of_items: count }),
            _ => return Err(CatalogCacheError::new("catalogue serving index returned an unknown facet")),
        }
    }
    let book_formats = super::CatalogBookFormat::ALL
        .into_iter()
        .filter_map(|value| {
            format_counts
                .iter()
                .find(|(format, _)| *format == value)
                .map(|(_, count)| CatalogBookFormatFacetValue { value, number_of_items: *count })
                .or_else(|| selected_formats.contains(&value).then_some(CatalogBookFormatFacetValue { value, number_of_items: 0 }))
        })
        .collect();
    for selected in selected_languages {
        let value = selected.split('-').next().unwrap_or(selected).to_ascii_lowercase();
        if !value.is_empty() && !languages.iter().any(|language| language.value == value) {
            languages.push(CatalogLanguageFacetValue { value, number_of_items: 0 });
        }
    }
    languages.sort_by(|left, right| left.value.cmp(&right.value));
    Ok(CatalogSubjectBrowseCapabilities { sorts: CatalogSubjectSort::ALL.to_vec(), book_formats, languages })
}

fn build_subject_format_capability_query(connection: &Connection, normalized_query: &str, subject_path: &str) -> Result<(String, String), CatalogCacheError> {
    let SubjectScopeQuery { matching_publications, from_clause, subject_filter, search_parameter } = build_subject_scope_query(connection, normalized_query, subject_path)?;
    let sql = format!(
        "{matching_publications}
         SELECT format.book_format,COUNT(*)
           {from_clause}
           JOIN catalog_source active ON active.source_id=p.source_id AND active.active_generation=p.generation
           JOIN catalog_publication_format format
             ON format.source_id=p.source_id AND format.generation=p.generation
            AND format.edition_id=p.edition_id AND format.work_id=p.work_id
          WHERE p.importable=1 AND {SUBJECT_CANONICAL_EDITION_FILTER} AND {SUBJECT_SOURCE_FILTER} AND {SUBJECT_LANGUAGE_FILTER}
            AND length(?6)>=0 AND {subject_filter}
          GROUP BY format.book_format"
    );
    Ok((sql, search_parameter))
}

fn build_subject_language_capability_query(connection: &Connection, normalized_query: &str, subject_path: &str) -> Result<(String, String), CatalogCacheError> {
    let SubjectScopeQuery { matching_publications, from_clause, subject_filter, search_parameter } = build_subject_scope_query(connection, normalized_query, subject_path)?;
    let sql = format!(
        "{matching_publications}
         SELECT lower(substr(language.language,1,instr(language.language || '-', '-')-1)) AS base_language,
                COUNT(DISTINCT p.source_id || char(31) || p.edition_id || char(31) || p.work_id)
           {from_clause}
           JOIN catalog_source active ON active.source_id=p.source_id AND active.active_generation=p.generation
           JOIN catalog_publication_language language
             ON language.source_id=p.source_id AND language.generation=p.generation
            AND language.edition_id=p.edition_id AND language.work_id=p.work_id
          WHERE p.importable=1 AND {SUBJECT_CANONICAL_EDITION_FILTER} AND {SUBJECT_SOURCE_FILTER} AND length(?5)>=0 AND {SUBJECT_FORMAT_FILTER}
            AND {subject_filter}
          GROUP BY base_language ORDER BY base_language"
    );
    Ok((sql, search_parameter))
}

fn build_subject_publication_query(connection: &Connection, normalized_query: &str, subject_path: &str, sort: CatalogSubjectSort, include_payloads: bool) -> Result<SubjectPublicationQuery, CatalogCacheError> {
    let SubjectScopeQuery { matching_publications, from_clause, subject_filter, search_parameter } = build_subject_scope_query(connection, normalized_query, subject_path)?;
    let ordering = match sort {
        CatalogSubjectSort::Popularity => {
            "(COALESCE(pop.reading_log_count,0)>0 OR COALESCE(pop.rating_count,0)>0) DESC,
             COALESCE(pop.reading_log_count,0) DESC,COALESCE(pop.rating_count,0) DESC,
             p.provider_popularity DESC,p.sequence DESC,p.normalized_title,p.source_id,p.edition_id,p.work_id"
        }
        CatalogSubjectSort::Title => "p.normalized_title,p.source_id,p.edition_id,p.work_id",
    };
    let selection = if include_payloads { "p.source_id,p.payload,p.entry_id" } else { "p.entry_id" };
    let sql = format!(
        "{matching_publications}
         SELECT {selection}
           {from_clause}
           JOIN catalog_source s ON s.source_id=p.source_id AND s.active_generation=p.generation
           LEFT JOIN catalog_publication_popularity pop
             ON pop.source_id=p.source_id AND pop.generation=p.generation
            AND pop.edition_id=p.edition_id AND pop.work_id=p.work_id
          WHERE p.importable=1 AND {SUBJECT_CANONICAL_EDITION_FILTER} AND {SUBJECT_SOURCE_FILTER} AND {SUBJECT_LANGUAGE_FILTER} AND {SUBJECT_FORMAT_FILTER}
            AND {subject_filter}
          ORDER BY {ordering}
          LIMIT ?7 OFFSET ?8"
    );
    Ok(SubjectPublicationQuery { sql, search_parameter })
}

fn build_subject_publication_count_query(connection: &Connection, normalized_query: &str, subject_path: &str) -> Result<SubjectPublicationCountQuery, CatalogCacheError> {
    let SubjectScopeQuery { matching_publications, from_clause, subject_filter, search_parameter } = build_subject_scope_query(connection, normalized_query, subject_path)?;
    let sql = format!(
        "{matching_publications}
         SELECT COUNT(*)
           {from_clause}
           JOIN catalog_source s ON s.source_id=p.source_id AND s.active_generation=p.generation
          WHERE p.importable=1 AND {SUBJECT_CANONICAL_EDITION_FILTER} AND {SUBJECT_SOURCE_FILTER} AND {SUBJECT_LANGUAGE_FILTER} AND {SUBJECT_FORMAT_FILTER}
            AND {subject_filter}"
    );
    Ok(SubjectPublicationCountQuery { sql, search_parameter })
}

#[cfg(test)]
pub(crate) fn materialized_subject_publication_query_plan(connection: &Connection, subject_path: &str) -> Result<Vec<String>, CatalogCacheError> {
    let query = build_subject_publication_query(connection, "", subject_path, CatalogSubjectSort::default(), false)?;
    let mut statement = connection.prepare(&format!("EXPLAIN QUERY PLAN {}", query.sql))?;
    let rows = statement.query_map(params![subject_projection::UNIFIED_SYSTEM_ID, subject_path, "", "[]", "[]", "[]", 20_i64, 0_i64], |row| row.get::<_, String>(3))?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

#[cfg(test)]
pub(crate) fn materialized_subject_capability_query_plans(connection: &Connection, subject_path: &str) -> Result<(Vec<String>, Vec<String>), CatalogCacheError> {
    let explain = |sql: String, search_parameter: String| -> Result<Vec<String>, CatalogCacheError> {
        let mut statement = connection.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
        let rows = statement.query_map(params![subject_projection::UNIFIED_SYSTEM_ID, subject_path, search_parameter, "[]", "[]", "[]"], |row| row.get::<_, String>(3))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    };
    let (format_sql, format_search_parameter) = build_subject_format_capability_query(connection, "", subject_path)?;
    let (language_sql, language_search_parameter) = build_subject_language_capability_query(connection, "", subject_path)?;
    Ok((explain(format_sql, format_search_parameter)?, explain(language_sql, language_search_parameter)?))
}

fn load_subject_publications(connection: &Connection, browse: &SubjectBrowseQuery<'_>, include_payloads: bool) -> Result<SubjectPublicationPage, CatalogCacheError> {
    let mut publications = Vec::new();
    if !browse.normalized_query.is_empty() || !browse.subject_path.is_empty() {
        if !browse.normalized_query.is_empty() && browse_serving_index_ready(connection)? {
            return load_serving_subject_publications(connection, browse, include_payloads);
        }
        let count = build_subject_publication_count_query(connection, browse.normalized_query, browse.subject_path)?;
        let count_started = Instant::now();
        let total = connection.query_row(&count.sql, params![browse.system_id, browse.subject_path, count.search_parameter, browse.source_ids, browse.supported_languages, browse.book_formats], |row| row.get::<_, i64>(0))?;
        let total = usize::try_from(stored_u64(total, "subject result count")?).unwrap_or(usize::MAX);
        log::info!("catalog_subject_browse phase=count results={} elapsed_ms={}", total, count_started.elapsed().as_millis());
        let query = build_subject_publication_query(connection, browse.normalized_query, browse.subject_path, browse.sort, include_payloads)?;
        let page_started = Instant::now();
        let mut statement = connection.prepare(&query.sql)?;
        let parameters = params![
            browse.system_id,
            browse.subject_path,
            query.search_parameter,
            browse.source_ids,
            browse.supported_languages,
            browse.book_formats,
            storage_i64(browse.limit as u64, "subject result limit")?,
            storage_i64(browse.offset as u64, "subject result offset")?
        ];
        if include_payloads {
            let rows = statement.query_map(parameters, |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?, row.get::<_, i64>(2)?)))?;
            for row in rows {
                let (source, payload, entry_id) = row?;
                let publication = decode_cached(&payload)?;
                publications.push(SequencedSubjectPublication {
                    item: Some(CatalogSourcedPublication { source_id: CatalogSourceId::parse(&source).map_err(CatalogCacheError::new)?, publication }),
                    entry_id: stored_u64(entry_id, "subject publication entry ID")?,
                    exact_assignment: true,
                    search_terms: Vec::new(),
                });
            }
        } else {
            let rows = statement.query_map(parameters, |row| row.get::<_, i64>(0))?;
            for row in rows {
                publications.push(SequencedSubjectPublication { item: None, entry_id: stored_u64(row?, "subject publication entry ID")?, exact_assignment: true, search_terms: Vec::new() });
            }
        }
        log::info!("catalog_subject_browse phase=ordered_page payloads={} returned={} elapsed_ms={}", include_payloads, publications.len(), page_started.elapsed().as_millis());
        return Ok(SubjectPublicationPage { publications, total });
    }
    Ok(SubjectPublicationPage { publications, total: 0 })
}

fn load_serving_subject_publications(connection: &Connection, browse: &SubjectBrowseQuery<'_>, include_payloads: bool) -> Result<SubjectPublicationPage, CatalogCacheError> {
    let query = build_serving_subject_publication_query(browse.normalized_query, browse.sort, include_payloads);
    let started = Instant::now();
    let mut statement = connection.prepare(&query.sql)?;
    let parameters = params![
        browse.system_id,
        browse.subject_path,
        query.search_parameter,
        browse.source_ids,
        browse.supported_languages,
        browse.book_formats,
        storage_i64(browse.limit as u64, "subject result limit")?,
        storage_i64(browse.offset as u64, "subject result offset")?
    ];
    let mut publications = Vec::new();
    let mut total = 0_usize;
    if include_payloads {
        let rows = statement.query_map(parameters, |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?, row.get::<_, i64>(2)?, row.get::<_, i64>(3)?)))?;
        for row in rows {
            let (source, payload, entry_id, row_total) = row?;
            total = usize::try_from(stored_u64(row_total, "subject result count")?).unwrap_or(usize::MAX);
            publications.push(SequencedSubjectPublication {
                item: Some(CatalogSourcedPublication { source_id: CatalogSourceId::parse(&source).map_err(CatalogCacheError::new)?, publication: decode_cached(&payload)? }),
                entry_id: stored_u64(entry_id, "subject publication entry ID")?,
                exact_assignment: true,
                search_terms: Vec::new(),
            });
        }
    } else {
        let rows = statement.query_map(parameters, |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
        for row in rows {
            let (entry_id, row_total) = row?;
            total = usize::try_from(stored_u64(row_total, "subject result count")?).unwrap_or(usize::MAX);
            publications.push(SequencedSubjectPublication { item: None, entry_id: stored_u64(entry_id, "subject publication entry ID")?, exact_assignment: true, search_terms: Vec::new() });
        }
    }
    if publications.is_empty() && browse.offset > 0 {
        let count = build_serving_subject_count_query(browse.normalized_query);
        let stored_total = connection.query_row(&count.sql, params![browse.system_id, browse.subject_path, count.search_parameter, browse.source_ids, browse.supported_languages, browse.book_formats], |row| row.get::<_, i64>(0))?;
        total = usize::try_from(stored_u64(stored_total, "subject result count")?).unwrap_or(usize::MAX);
    }
    log::info!("catalog_subject_browse phase=serving_page payloads={} results={} returned={} elapsed_ms={}", include_payloads, total, publications.len(), started.elapsed().as_millis());
    Ok(SubjectPublicationPage { publications, total })
}

fn load_subject_collection_publications(connection: &Connection, system_id: &str, subject_path: &str, limit: usize, offset: usize) -> Result<SubjectPublicationPage, CatalogCacheError> {
    let materialized = subject_browse_index_ready(connection)?;
    let (membership_table, path_filter, exact_expression) = if materialized {
        ("catalog_publication_subject_node", "(?2='' OR membership.node_path=?2)", "MAX(CASE WHEN ?2!='' AND membership.node_path=?2 THEN membership.exact_assignment ELSE 0 END)")
    } else {
        ("catalog_publication_subject_assignment", "(?2='' OR membership.subject_path=?2 OR instr(membership.subject_path,?2 || ' / ')=1)", "MAX(CASE WHEN ?2!='' AND membership.subject_path=?2 THEN 1 ELSE 0 END)")
    };
    let base = format!(
        "FROM {membership_table} membership
         JOIN catalog_source active
           ON active.source_id=membership.source_id AND active.active_generation=membership.generation
         JOIN catalog_publication p
           ON p.source_id=membership.source_id AND p.generation=membership.generation
          AND p.edition_id=membership.edition_id AND p.work_id=membership.work_id
         LEFT JOIN catalog_publication_popularity pop
           ON pop.source_id=p.source_id AND pop.generation=p.generation
          AND pop.edition_id=p.edition_id AND pop.work_id=p.work_id
        WHERE membership.system_id=?1 AND {path_filter} AND p.importable=1
          AND {SUBJECT_CANONICAL_EDITION_FILTER}"
    );
    let count_sql = format!(
        "SELECT COUNT(*) FROM (
             SELECT 1 {base}
             GROUP BY p.source_id,p.generation,p.edition_id,p.work_id
         )"
    );
    let total = connection.query_row(&count_sql, params![system_id, subject_path], |row| row.get::<_, i64>(0))?;
    let total = usize::try_from(stored_u64(total, "subject collection count")?).unwrap_or(usize::MAX);
    let sql = format!(
        "SELECT p.source_id,p.payload,p.entry_id,{exact_expression} AS exact_assignment,
                COALESCE((SELECT json_group_array(toc.normalized_title)
                            FROM catalog_publication_toc toc
                           WHERE toc.source_id=p.source_id AND toc.generation=p.generation
                             AND toc.edition_id=p.edition_id AND toc.work_id=p.work_id),'[]')
           {base}
          GROUP BY p.source_id,p.generation,p.edition_id,p.work_id
          ORDER BY (COALESCE(MAX(pop.reading_log_count),0)>0 OR COALESCE(MAX(pop.rating_count),0)>0) DESC,
                   COALESCE(MAX(pop.reading_log_count),0) DESC,COALESCE(MAX(pop.rating_count),0) DESC,
                   p.provider_popularity DESC,p.sequence DESC,p.normalized_title,p.source_id,p.edition_id,p.work_id
          LIMIT ?3 OFFSET ?4"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![system_id, subject_path, storage_i64(limit as u64, "subject collection limit")?, storage_i64(offset as u64, "subject collection offset")?], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?, row.get::<_, i64>(2)?, row.get::<_, bool>(3)?, row.get::<_, String>(4)?))
    })?;
    let mut publications = Vec::new();
    for row in rows {
        let (source, payload, entry_id, exact_assignment, search_terms) = row?;
        publications.push(SequencedSubjectPublication {
            item: Some(CatalogSourcedPublication { source_id: CatalogSourceId::parse(&source).map_err(CatalogCacheError::new)?, publication: decode_cached(&payload)? }),
            entry_id: stored_u64(entry_id, "subject publication entry ID")?,
            exact_assignment,
            search_terms: serde_json::from_str(&search_terms)?,
        });
    }
    Ok(SubjectPublicationPage { publications, total })
}

fn subject_range(offset: usize, returned_items: usize, total_items: usize) -> CatalogResultRangeResponse {
    CatalogResultRangeResponse { offset: u32::try_from(offset).unwrap_or(u32::MAX), returned_items: u32::try_from(returned_items).unwrap_or(u32::MAX), total_items: u32::try_from(total_items).unwrap_or(u32::MAX) }
}

pub(crate) fn valid_subject_key(system_id: &str, subject_path: &str) -> bool {
    !system_id.trim().is_empty()
        && system_id.len() <= 32
        && system_id.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !subject_path.trim().is_empty()
        && subject_path.len() <= 1_024
        && !subject_path.chars().any(char::is_control)
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SubjectBrowseLocation {
    system_id: Option<String>,
    subject_path: Option<String>,
}

pub(crate) fn browse_subject_system(system_id: &str) -> bool {
    system_id == subject_projection::UNIFIED_SYSTEM_ID
}

pub(crate) fn subject_locator(location: &SubjectBrowseLocation) -> String {
    let mut locator = url::Url::parse("bokheim:subjects").expect("static catalogue subject locator must be valid");
    {
        let mut query = locator.query_pairs_mut();
        if let Some(system_id) = &location.system_id {
            query.append_pair("system", system_id);
        }
        if let Some(subject_path) = &location.subject_path {
            query.append_pair("path", subject_path);
        }
    }
    locator.into()
}

pub(crate) fn parse_subject_locator(locator: &str) -> Result<SubjectBrowseLocation, CatalogCacheError> {
    if locator.len() > 2_048 {
        return Err(CatalogCacheError::new("catalogue subject locator is too long"));
    }
    let parsed = url::Url::parse(locator).map_err(|_| CatalogCacheError::new("invalid catalogue subject locator"))?;
    if parsed.scheme() != "bokheim" || parsed.path() != "subjects" || parsed.fragment().is_some() {
        return Err(CatalogCacheError::new("invalid catalogue subject locator"));
    }
    let mut location = SubjectBrowseLocation::default();
    for (key, value) in parsed.query_pairs() {
        match key.as_ref() {
            "system" if location.system_id.is_none() => location.system_id = Some(value.into_owned().to_ascii_lowercase()),
            "path" if location.subject_path.is_none() => location.subject_path = Some(value.into_owned()),
            _ => return Err(CatalogCacheError::new("invalid catalogue subject locator")),
        }
    }
    let Some(system_id) = location.system_id.as_deref() else { return Err(CatalogCacheError::new("catalogue subject locator is missing its system")) };
    if !browse_subject_system(system_id) || location.subject_path.as_deref().is_some_and(|path| !valid_subject_key(system_id, path)) {
        return Err(CatalogCacheError::new("invalid catalogue subject locator"));
    }
    Ok(location)
}

pub(crate) fn canonical_subject_locator(locator: &str) -> Result<String, CatalogCacheError> {
    Ok(subject_locator(&parse_subject_locator(locator)?))
}
