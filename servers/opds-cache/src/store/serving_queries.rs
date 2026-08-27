use super::subject_browse::{CatalogSequencedSubjectPublication, CatalogSubjectBrowseQuery, CatalogSubjectPublicationPage};
use super::*;
use catalog_model::{CatalogBookFormat, CatalogBookFormatFacetValue, CatalogLanguageFacetValue, CatalogSourcedPublication, CatalogSubjectBrowseCapabilities, CatalogSubjectSort};

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

pub(super) fn load_publications_if_ready(connection: &Connection, query: &CatalogSubjectBrowseQuery<'_>, include_payloads: bool) -> Result<Option<CatalogSubjectPublicationPage>, CatalogCacheError> {
    if !serving_index_ready(connection)? {
        return Ok(None);
    }
    load_publications(connection, query, include_payloads).map(Some)
}

pub(super) fn load_capabilities_if_ready(
    connection: &Connection, query: &CatalogSubjectBrowseQuery<'_>, selected_formats: &[CatalogBookFormat], selected_languages: &[String],
) -> Result<Option<CatalogSubjectBrowseCapabilities>, CatalogCacheError> {
    if !serving_index_ready(connection)? {
        return Ok(None);
    }
    load_capabilities(connection, query, selected_formats, selected_languages).map(Some)
}

fn serving_index_ready(connection: &Connection) -> Result<bool, CatalogCacheError> {
    connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM catalog_browse_serving_metadata metadata
                WHERE metadata.singleton=1 AND metadata.index_version=?1
                  AND metadata.ready=1)",
            [CATALOG_BROWSE_SERVING_INDEX_VERSION],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

struct ServingQuery {
    sql: String,
    search_parameter: String,
}

fn search_parameter(normalized_query: &str) -> String {
    format!("\"{}\"", normalized_query.replace('"', "\"\""))
}

fn publication_query(normalized_query: &str, sort: CatalogSubjectSort, include_payloads: bool) -> ServingQuery {
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
    ServingQuery { sql, search_parameter: search_parameter(normalized_query) }
}

fn count_query(normalized_query: &str) -> ServingQuery {
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
    ServingQuery { sql, search_parameter: search_parameter(normalized_query) }
}

fn capabilities_query(normalized_query: &str) -> ServingQuery {
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
    ServingQuery { sql, search_parameter: search_parameter(normalized_query) }
}

fn load_publications(connection: &Connection, browse: &CatalogSubjectBrowseQuery<'_>, include_payloads: bool) -> Result<CatalogSubjectPublicationPage, CatalogCacheError> {
    let query = publication_query(browse.normalized_query, browse.sort, include_payloads);
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
            publications.push(CatalogSequencedSubjectPublication {
                item: Some(CatalogSourcedPublication { source_id: CatalogSourceId::parse(&source).map_err(CatalogCacheError::new)?, publication: decode_cached(&payload)? }),
                entry_id: stored_u64(entry_id, "subject publication entry ID")?,
            });
        }
    } else {
        let rows = statement.query_map(parameters, |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
        for row in rows {
            let (entry_id, row_total) = row?;
            total = usize::try_from(stored_u64(row_total, "subject result count")?).unwrap_or(usize::MAX);
            publications.push(CatalogSequencedSubjectPublication { item: None, entry_id: stored_u64(entry_id, "subject publication entry ID")? });
        }
    }
    if publications.is_empty() && browse.offset > 0 {
        let count = count_query(browse.normalized_query);
        let stored_total = connection.query_row(&count.sql, params![browse.system_id, browse.subject_path, count.search_parameter, browse.source_ids, browse.supported_languages, browse.book_formats], |row| row.get::<_, i64>(0))?;
        total = usize::try_from(stored_u64(stored_total, "subject result count")?).unwrap_or(usize::MAX);
    }
    tracing::info!(include_payloads, total, returned = publications.len(), elapsed_ms = started.elapsed().as_millis(), "catalogue subject serving page loaded");
    Ok(CatalogSubjectPublicationPage { publications, total })
}

fn load_capabilities(connection: &Connection, query: &CatalogSubjectBrowseQuery<'_>, selected_formats: &[CatalogBookFormat], selected_languages: &[String]) -> Result<CatalogSubjectBrowseCapabilities, CatalogCacheError> {
    let capability_query = capabilities_query(query.normalized_query);
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
    let book_formats = CatalogBookFormat::ALL
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
