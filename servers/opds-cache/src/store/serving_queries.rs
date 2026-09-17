use super::subject_browse::{CatalogSequencedSubjectBook, CatalogSubjectBrowseQuery, CatalogSubjectBookPage};
use super::*;
use catalog_model::{CatalogBookFormat, CatalogBookFormatFacetValue, CatalogLanguageFacetValue, CatalogSourcedBook, CatalogSubjectBrowseCapabilities, CatalogSubjectSort};

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

pub(super) fn load_books_if_ready(connection: &Connection, query: &CatalogSubjectBrowseQuery<'_>, include_payloads: bool) -> Result<Option<CatalogSubjectBookPage>, CatalogStoreError> {
    if !serving_index_ready(connection)? {
        return Ok(None);
    }
    load_books(connection, query, include_payloads).map(Some)
}

pub(super) fn load_capabilities_if_ready(
    connection: &Connection, query: &CatalogSubjectBrowseQuery<'_>, selected_formats: &[CatalogBookFormat], selected_languages: &[String],
) -> Result<Option<CatalogSubjectBrowseCapabilities>, CatalogStoreError> {
    if !serving_index_ready(connection)? {
        return Ok(None);
    }
    load_capabilities(connection, query, selected_formats, selected_languages).map(Some)
}

pub(super) fn load_navigation_if_ready(connection: &Connection, query: &CatalogSubjectBrowseQuery<'_>) -> Result<Option<Vec<(String, String)>>, CatalogStoreError> {
    if !serving_index_ready(connection)? || !query.normalized_query.is_empty() {
        return Ok(None);
    }
    let sql = format!(
        "SELECT DISTINCT node.title,node.node_path
           FROM catalog_subject_node node
           JOIN catalog_browse_subject subject
             ON subject.system_id=node.system_id AND subject.node_path=node.node_path
           JOIN catalog_browse_book browse ON browse.entry_id=subject.entry_id
          WHERE node.system_id=?1 AND node.parent_path=?2
            AND {SERVING_SOURCE_FILTER} AND {SERVING_LANGUAGE_FILTER} AND {SERVING_FORMAT_FILTER}
          ORDER BY node.title"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map(params![query.system_id, query.subject_path, query.normalized_query, query.source_ids, query.supported_languages, query.book_formats], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    Ok(Some(rows.collect::<Result<Vec<_>, _>>()?))
}

fn serving_index_ready(connection: &Connection) -> Result<bool, CatalogStoreError> {
    // Legacy in-catalog serving tables are no longer maintained.
    if !connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='projection_state' AND type='table')", [], |r| r.get::<_, bool>(0))? {
        return Ok(false);
    }
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

fn book_query(normalized_query: &str, sort: CatalogSubjectSort, include_payloads: bool) -> ServingQuery {
    let ordering = match sort {
        CatalogSubjectSort::Popularity => {
            "(page.reading_log_count>0 OR page.rating_count>0) DESC,
             page.reading_log_count DESC,page.rating_count DESC,
             page.provider_popularity DESC,page.sequence DESC,
             page.normalized_title,page.source_id,page.edition_id,page.work_id"
        }
        CatalogSubjectSort::Title => "page.normalized_title,page.source_id,page.edition_id,page.work_id",
    };
    let selection = if include_payloads { "book.source_id,book.payload,page.entry_id" } else { "page.entry_id" };
    let payload_join = if include_payloads {
        "JOIN catalog_book book
           ON book.entry_id=page.entry_id"
    } else {
        ""
    };
    let scope = if normalized_query.is_empty() {
        format!(
            "WITH filtered AS MATERIALIZED (
                 SELECT browse.*
                   FROM catalog_browse_subject subject
                   JOIN catalog_browse_book browse ON browse.entry_id=subject.entry_id
                  WHERE subject.system_id=?1 AND subject.node_path=?2
                    AND {SERVING_SOURCE_FILTER} AND {SERVING_LANGUAGE_FILTER} AND {SERVING_FORMAT_FILTER}
             )"
        )
    } else {
        format!(
            "WITH matching(entry_id) AS MATERIALIZED (
                 SELECT rowid FROM catalog_browse_search_fts
                  WHERE catalog_browse_search_fts MATCH ?3
             ), filtered AS MATERIALIZED (
                 SELECT browse.*
                   FROM matching
                   JOIN catalog_browse_book browse ON browse.entry_id=matching.entry_id
                  WHERE {SERVING_SOURCE_FILTER} AND {SERVING_LANGUAGE_FILTER}
                    AND {SERVING_FORMAT_FILTER} AND {SERVING_SUBJECT_FILTER}
             )"
        )
    };
    let sql = format!(
        "{scope}, page AS MATERIALIZED (
             SELECT filtered.*
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

fn capabilities_query(normalized_query: &str) -> ServingQuery {
    if normalized_query.is_empty() {
        let language_filter = SERVING_LANGUAGE_FILTER.replace("browse.", "filtered.");
        let format_filter = SERVING_FORMAT_FILTER.replace("browse.", "filtered.");
        let sql = format!(
            "WITH filtered AS MATERIALIZED (
                 SELECT browse.entry_id,browse.has_language
                   FROM catalog_browse_subject subject
                   JOIN catalog_browse_book browse ON browse.entry_id=subject.entry_id
                  WHERE subject.system_id=?1 AND subject.node_path=?2 AND {SERVING_SOURCE_FILTER}
             )
             SELECT 'format',format.book_format,COUNT(*)
               FROM filtered JOIN catalog_browse_format format ON format.entry_id=filtered.entry_id
              WHERE {language_filter}
              GROUP BY format.book_format
             UNION ALL
             SELECT 'language',language.language,COUNT(*)
               FROM filtered JOIN catalog_browse_language language ON language.entry_id=filtered.entry_id
              WHERE {format_filter}
              GROUP BY language.language"
        );
        return ServingQuery { sql, search_parameter: String::new() };
    }
    let sql = format!(
        "WITH matching(entry_id) AS MATERIALIZED (
             SELECT rowid FROM catalog_browse_search_fts
              WHERE catalog_browse_search_fts MATCH ?3
         )
         SELECT 'format',format.book_format,COUNT(*)
           FROM matching
           JOIN catalog_browse_book browse ON browse.entry_id=matching.entry_id
           JOIN catalog_browse_format format ON format.entry_id=browse.entry_id
          WHERE {SERVING_SOURCE_FILTER} AND {SERVING_LANGUAGE_FILTER}
            AND {SERVING_SUBJECT_FILTER}
          GROUP BY format.book_format
         UNION ALL
         SELECT 'language',language.language,COUNT(*)
           FROM matching
           JOIN catalog_browse_book browse ON browse.entry_id=matching.entry_id
           JOIN catalog_browse_language language ON language.entry_id=browse.entry_id
          WHERE {SERVING_SOURCE_FILTER} AND {SERVING_FORMAT_FILTER}
            AND {SERVING_SUBJECT_FILTER}
          GROUP BY language.language"
    );
    ServingQuery { sql, search_parameter: search_parameter(normalized_query) }
}

fn load_books(connection: &Connection, browse: &CatalogSubjectBrowseQuery<'_>, include_payloads: bool) -> Result<CatalogSubjectBookPage, CatalogStoreError> {
    let query = book_query(browse.normalized_query, browse.sort, include_payloads);
    let started = Instant::now();
    let mut statement = connection.prepare(&query.sql)?;
    let parameters = params![
        browse.system_id,
        browse.subject_path,
        query.search_parameter,
        browse.source_ids,
        browse.supported_languages,
        browse.book_formats,
        storage_i64(browse.limit.saturating_add(1) as u64, "subject result limit")?,
        storage_i64(browse.offset as u64, "subject result offset")?
    ];
    let mut books = Vec::new();
    if include_payloads {
        let rows = statement.query_map(parameters, |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?, row.get::<_, i64>(2)?)))?;
        for row in rows {
            let (source, payload, entry_id) = row?;
            books.push(CatalogSequencedSubjectBook {
                item: Some(CatalogSourcedBook { source_id: CatalogSourceId::parse(&source).map_err(CatalogStoreError::new)?, book: decode_cached(&payload)? }),
                entry_id: stored_u64(entry_id, "subject book entry ID")?,
            });
        }
    } else {
        let rows = statement.query_map(parameters, |row| row.get::<_, i64>(0))?;
        for row in rows {
            books.push(CatalogSequencedSubjectBook { item: None, entry_id: stored_u64(row?, "subject book entry ID")? });
        }
    }
    let has_more = books.len() > browse.limit;
    books.truncate(browse.limit);
    tracing::info!(include_payloads, has_more, returned = books.len(), elapsed_ms = started.elapsed().as_millis(), "catalogue subject serving page loaded");
    Ok(CatalogSubjectBookPage { books, has_more })
}

fn load_capabilities(connection: &Connection, query: &CatalogSubjectBrowseQuery<'_>, selected_formats: &[CatalogBookFormat], selected_languages: &[String]) -> Result<CatalogSubjectBrowseCapabilities, CatalogStoreError> {
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
            _ => return Err(CatalogStoreError::new("catalogue serving index returned an unknown facet")),
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
