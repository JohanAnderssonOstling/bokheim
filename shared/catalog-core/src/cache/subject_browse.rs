use super::{
    decode_cached, normalize_search_text, params, project_gutenberg_download_count, storage_i64, stored_u64, CatalogCache, CatalogCacheError, CatalogLink, CatalogLinkTarget, CatalogPagination, CatalogPublication, CatalogSourceId,
    CatalogSourcedPublication, CatalogSubjectBrowseRequest, CatalogSubjectBrowseResponse, Connection, CATALOG_SEARCH_INDEX_VERSION,
};

impl CatalogCache {
    /// Browses the same unified taxonomy projection as the local library. The
    /// locator is deliberately opaque to clients so taxonomy storage and
    /// hierarchy rules stay here rather than being duplicated in each UI.
    pub fn browse_subjects(&self, request: &CatalogSubjectBrowseRequest) -> Result<CatalogSubjectBrowseResponse, CatalogCacheError> {
        const MAX_RESULTS: usize = 100;
        const MAX_SOURCE_FILTERS: usize = 256;
        if request.source_ids.len() > MAX_SOURCE_FILTERS {
            return Err(CatalogCacheError::new("too many catalogue source filters"));
        }
        let limit = usize::try_from(request.limit).unwrap_or(MAX_RESULTS).clamp(1, MAX_RESULTS);
        let normalized_query = normalize_search_text(&request.query);
        let location = request.locator.as_deref().map(parse_subject_locator).transpose()?.unwrap_or_else(|| SubjectBrowseLocation { system_id: Some(library_model::UNIFIED_SYSTEM_ID.to_owned()), subject_path: None, offset: 0 });
        let offset = usize::try_from(location.offset).unwrap_or(usize::MAX);
        let connection = self.pool.get()?;
        let system_id = location.system_id.as_deref().expect("subject system checked above");
        if !browse_subject_system(system_id) {
            return Err(CatalogCacheError::new("unsupported catalogue subject system"));
        }
        let subject_path = location.subject_path.as_deref().unwrap_or("");
        let source_ids = serde_json::to_string(&request.source_ids.iter().map(CatalogSourceId::as_str).collect::<Vec<_>>())?;
        let supported_languages = serde_json::to_string(&request.supported_languages)?;
        let query = SubjectBrowseQuery { system_id, subject_path, normalized_query: &normalized_query, source_ids: &source_ids, supported_languages: &supported_languages, limit, offset };
        let navigation = load_subject_navigation(&connection, &query)?;
        let results = load_subject_publications(&connection, &query)?;
        let pagination = subject_pagination(&location, offset, limit, results.total, results.has_more);
        Ok(CatalogSubjectBrowseResponse { navigation, publications: results.publications, pagination })
    }
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

struct SubjectPublicationPage {
    publications: Vec<CatalogSourcedPublication>,
    total: Option<usize>,
    has_more: bool,
}

struct SubjectBrowseQuery<'a> {
    system_id: &'a str,
    subject_path: &'a str,
    normalized_query: &'a str,
    source_ids: &'a str,
    supported_languages: &'a str,
    limit: usize,
    offset: usize,
}

fn load_subject_navigation(connection: &Connection, query: &SubjectBrowseQuery<'_>) -> Result<Vec<CatalogLink>, CatalogCacheError> {
    let source_filter = SUBJECT_SOURCE_FILTER;
    let language_filter = SUBJECT_LANGUAGE_FILTER;
    let mut navigation = Vec::new();
    if query.normalized_query.is_empty() {
        let sql = format!(
            "WITH scoped AS (
                SELECT CASE WHEN ?2='' THEN a.subject_path ELSE substr(a.subject_path,length(?2)+4) END AS remainder,
                       p.source_id || char(31) || p.edition_id || char(31) || p.work_id AS publication_key
                  FROM catalog_publication_subject_assignment a
                  JOIN catalog_source s ON s.source_id=a.source_id AND s.active_generation=a.generation
                  JOIN catalog_publication p ON p.source_id=a.source_id AND p.generation=a.generation
                   AND p.edition_id=a.edition_id AND p.work_id=a.work_id
                 WHERE a.system_id=?1 AND p.importable=1
                   AND (?2='' OR instr(a.subject_path,?2 || ' / ')=1)
                   AND {source_filter} AND {language_filter}
             ), children AS (
                SELECT CASE WHEN instr(remainder,' / ')>0 THEN substr(remainder,1,instr(remainder,' / ')-1) ELSE remainder END AS child,
                       publication_key
                  FROM scoped WHERE remainder!=''
             )
             SELECT child,COUNT(DISTINCT publication_key) FROM children
              WHERE child!='' GROUP BY child ORDER BY child"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params![query.system_id, query.subject_path, query.normalized_query, query.source_ids, query.supported_languages], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
        for row in rows {
            let (title, count) = row?;
            let child_path = if query.subject_path.is_empty() { title.clone() } else { format!("{} / {title}", query.subject_path) };
            navigation.push(CatalogLink {
                title,
                target: CatalogLinkTarget::Page { locator: subject_locator(&SubjectBrowseLocation { system_id: Some(query.system_id.to_owned()), subject_path: Some(child_path), offset: 0 }) },
                number_of_items: Some(u32::try_from(stored_u64(count, "subject publication count")?).unwrap_or(u32::MAX)),
                active: false,
            });
        }
    }

    Ok(navigation)
}

struct SubjectPublicationQuery {
    sql: String,
    search_parameter: String,
    exact_total: bool,
    fetch_limit: usize,
}

fn build_subject_publication_query(connection: &Connection, normalized_query: &str, limit: usize) -> Result<SubjectPublicationQuery, CatalogCacheError> {
    let source_filter = SUBJECT_SOURCE_FILTER;
    let language_filter = SUBJECT_LANGUAGE_FILTER;
    let path_filter = if normalized_query.is_empty() { "a.subject_path=?2" } else { "(?2='' OR a.subject_path=?2 OR instr(a.subject_path,?2 || ' / ')=1)" };
    let use_search_index = !normalized_query.is_empty()
        && normalized_query.chars().count() >= 3
        && connection.query_row("SELECT index_version=?1 AND ready=1 FROM catalog_search_index_metadata WHERE singleton=1", [CATALOG_SEARCH_INDEX_VERSION], |row| row.get::<_, bool>(0))?;
    // Quoting makes the normalized text a literal FTS phrase instead of
    // exposing MATCH operators supplied by an HTTP client. The trigram
    // tokenizer preserves the previous substring-search semantics.
    let search_parameter = if use_search_index { format!("\"{}\"", normalized_query.replace('"', "\"\"")) } else { normalized_query.to_owned() };
    let exact_total = normalized_query.is_empty();
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
    let matching_join = if normalized_query.is_empty() {
        String::new()
    } else {
        "JOIN matching_publications matching
            ON matching.source_id=p.source_id AND matching.generation=p.generation
           AND matching.edition_id=p.edition_id AND matching.work_id=p.work_id"
            .to_owned()
    };
    let count_projection = if exact_total { "COUNT(*) OVER()" } else { "NULL" };
    let fetch_limit = if exact_total { limit } else { limit.saturating_add(1) };
    let sql = format!(
        "{matching_publications}
         SELECT p.source_id,p.payload,{count_projection},COALESCE(pop.reading_log_count,0),COALESCE(pop.rating_count,0),p.sequence
           FROM catalog_publication p
           JOIN catalog_source s ON s.source_id=p.source_id AND s.active_generation=p.generation
           {matching_join}
           LEFT JOIN catalog_publication_popularity pop
             ON pop.source_id=p.source_id AND pop.generation=p.generation
            AND pop.edition_id=p.edition_id AND pop.work_id=p.work_id
          WHERE p.importable=1 AND {source_filter} AND {language_filter}
            AND EXISTS(SELECT 1 FROM catalog_publication_subject_assignment a
                 WHERE a.source_id=p.source_id AND a.generation=p.generation
                   AND a.edition_id=p.edition_id AND a.work_id=p.work_id
                   AND a.system_id=?1 AND {path_filter})
          ORDER BY (COALESCE(pop.reading_log_count,0)>0 OR COALESCE(pop.rating_count,0)>0) DESC,
                   COALESCE(pop.reading_log_count,0) DESC,COALESCE(pop.rating_count,0) DESC,p.sequence DESC,
                   p.source_id,p.edition_id,p.work_id
          LIMIT ?6 OFFSET ?7"
    );
    Ok(SubjectPublicationQuery { sql, search_parameter, exact_total, fetch_limit })
}

fn load_subject_publications(connection: &Connection, browse: &SubjectBrowseQuery<'_>) -> Result<SubjectPublicationPage, CatalogCacheError> {
    let mut ranked_publications = Vec::new();
    let mut total = None;
    let mut has_more = false;
    if !browse.normalized_query.is_empty() || !browse.subject_path.is_empty() {
        let query = build_subject_publication_query(connection, browse.normalized_query, browse.limit)?;
        let exact_total = query.exact_total;
        let mut statement = connection.prepare(&query.sql)?;
        let rows = statement.query_map(
            params![
                browse.system_id,
                browse.subject_path,
                query.search_parameter,
                browse.source_ids,
                browse.supported_languages,
                storage_i64(query.fetch_limit as u64, "subject result limit")?,
                storage_i64(browse.offset as u64, "subject result offset")?
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?, row.get::<_, Option<i64>>(2)?, row.get::<_, i64>(3)?, row.get::<_, i64>(4)?, row.get::<_, i64>(5)?)),
        )?;
        for row in rows {
            let (source, payload, count, reading_log_count, rating_count, sequence) = row?;
            if let Some(count) = count {
                total = Some(usize::try_from(stored_u64(count, "subject result count")?).unwrap_or(usize::MAX));
            }
            if !exact_total && ranked_publications.len() == browse.limit {
                has_more = true;
                continue;
            }
            let publication: CatalogPublication = decode_cached(&payload)?;
            let provider_popularity = project_gutenberg_download_count(&publication).unwrap_or(0);
            ranked_publications.push((
                CatalogSourcedPublication { source_id: CatalogSourceId::parse(&source).map_err(CatalogCacheError::new)?, publication },
                (reading_log_count > 0 || rating_count > 0, reading_log_count, rating_count, provider_popularity, sequence),
            ));
        }
    }
    ranked_publications.sort_by(|left, right| right.1.cmp(&left.1));
    let publications = ranked_publications.into_iter().map(|(publication, _)| publication).collect();
    Ok(SubjectPublicationPage { publications, total, has_more })
}

fn subject_pagination(location: &SubjectBrowseLocation, offset: usize, limit: usize, total: Option<usize>, has_more: bool) -> CatalogPagination {
    let page = offset / limit + 1;
    let at = |new_offset| subject_locator(&SubjectBrowseLocation { offset: u32::try_from(new_offset).unwrap_or(u32::MAX), ..location.clone() });
    let next = if total.map_or(has_more, |total| offset.saturating_add(limit) < total) { Some(at(offset.saturating_add(limit))) } else { None };
    let last = total.and_then(|total| {
        let page_count = total.div_ceil(limit).max(1);
        (page < page_count).then(|| at((page_count - 1) * limit))
    });
    CatalogPagination {
        first: (offset > 0).then(|| at(0)),
        previous: (offset > 0).then(|| at(offset.saturating_sub(limit))),
        next,
        last,
        number_of_items: total.map(|total| u32::try_from(total).unwrap_or(u32::MAX)),
        items_per_page: Some(u32::try_from(limit).unwrap_or(u32::MAX)),
        current_page: Some(u32::try_from(page).unwrap_or(u32::MAX)),
    }
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
    offset: u32,
}

pub(crate) fn browse_subject_system(system_id: &str) -> bool {
    system_id == library_model::UNIFIED_SYSTEM_ID
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
        if location.offset > 0 {
            query.append_pair("offset", &location.offset.to_string());
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
            "offset" if location.offset == 0 => location.offset = value.parse().map_err(|_| CatalogCacheError::new("invalid catalogue subject offset"))?,
            _ => return Err(CatalogCacheError::new("invalid catalogue subject locator")),
        }
    }
    let Some(system_id) = location.system_id.as_deref() else { return Err(CatalogCacheError::new("catalogue subject locator is missing its system")) };
    if !browse_subject_system(system_id) || location.subject_path.as_deref().is_some_and(|path| !valid_subject_key(system_id, path)) {
        return Err(CatalogCacheError::new("invalid catalogue subject locator"));
    }
    Ok(location)
}

pub(crate) fn subject_distance(left: &str, right: &str) -> usize {
    let left = left.split(" / ").count();
    let right = right.split(" / ").count();
    left.abs_diff(right)
}

pub(crate) fn language_supported(publication_language: &str, supported: &[String]) -> bool {
    supported
        .iter()
        .any(|supported| publication_language.eq_ignore_ascii_case(supported) || publication_language.split('-').next().is_some_and(|primary| supported.split('-').next().is_some_and(|candidate| primary.eq_ignore_ascii_case(candidate))))
}
