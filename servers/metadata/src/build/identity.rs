use crate::build::batch::{load_work_codes_cached, WorkEvidenceCache};
use crate::build::import::core::author_match_keys;
use crate::Isbn13;
use crate::{
    classification_similarity_keys, error, sql_placeholders, BTreeMap, BTreeSet, Classification, ClassificationScheme, ClassificationSource, Connection, EditionIdentityMatch, EditionTitleMatch, HashMap, MetadataError,
    BULK_EVIDENCE_IDS_PER_QUERY, LCC_SCHEME,
};
use rusqlite::{params_from_iter, OptionalExtension};

struct EditionCandidate {
    edition_id: i64,
    work_id: Option<i64>,
    edition_title: Option<String>,
    subtitle: Option<String>,
    work_title: Option<String>,
    book_year: Option<i32>,
}

fn load_edition_authors(connection: &Connection, edition_ids: &BTreeSet<i64>) -> Result<HashMap<i64, Vec<String>>, MetadataError> {
    load_names_by_ids(connection, edition_ids, "edition_author", "edition_id")
}

fn load_work_authors(connection: &Connection, work_ids: &BTreeSet<i64>) -> Result<HashMap<i64, Vec<String>>, MetadataError> {
    load_names_by_ids(connection, work_ids, "work_author", "work_id")
}

pub(crate) fn load_names_by_ids(connection: &Connection, ids: &BTreeSet<i64>, relation_table: &str, id_column: &str) -> Result<HashMap<i64, Vec<String>>, MetadataError> {
    let mut names = HashMap::<i64, Vec<String>>::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(BULK_EVIDENCE_IDS_PER_QUERY) {
        let sql = format!(
            "SELECT reference.{id_column},author.name FROM {relation_table} reference JOIN author ON author.author_id=reference.author_id WHERE reference.{id_column} IN ({}) AND author.name IS NOT NULL ORDER BY reference.{id_column},reference.position",
            sql_placeholders(chunk.len())
        );
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).map_err(error)?;
        for row in rows {
            let (id, name) = row.map_err(error)?;
            names.entry(id).or_default().push(name);
        }
    }
    Ok(names)
}

fn load_edition_publishers(connection: &Connection, edition_ids: &BTreeSet<i64>) -> Result<HashMap<i64, Vec<(String, String)>>, MetadataError> {
    let mut publishers = HashMap::<i64, Vec<(String, String)>>::new();
    for chunk in edition_ids.iter().copied().collect::<Vec<_>>().chunks(BULK_EVIDENCE_IDS_PER_QUERY) {
        let sql = format!("SELECT edition_id,name,normalized_name FROM edition_publisher WHERE edition_id IN ({}) ORDER BY edition_id,position", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))).map_err(error)?;
        for row in rows {
            let (edition_id, name, normalized_name) = row.map_err(error)?;
            publishers.entry(edition_id).or_default().push((name, normalized_name));
        }
    }
    Ok(publishers)
}

fn load_edition_isbns(connection: &Connection, edition_ids: &BTreeSet<i64>) -> Result<HashMap<i64, Isbn13>, MetadataError> {
    let mut isbns = HashMap::<i64, Isbn13>::new();
    for chunk in edition_ids.iter().copied().collect::<Vec<_>>().chunks(BULK_EVIDENCE_IDS_PER_QUERY) {
        let sql = format!("SELECT edition_id,isbn13 FROM edition_isbn WHERE edition_id IN ({}) ORDER BY edition_id,isbn13", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Isbn13>(1)?))).map_err(error)?;
        for row in rows {
            let (edition_id, isbn) = row.map_err(error)?;
            isbns.entry(edition_id).or_insert(isbn);
        }
    }
    Ok(isbns)
}

pub(crate) fn load_classifications_by_ids(connection: &Connection, table: &str, id_column: &str, ids: &BTreeSet<i64>) -> Result<HashMap<i64, Vec<(ClassificationScheme, String)>>, MetadataError> {
    let mut classifications = HashMap::<i64, Vec<(ClassificationScheme, String)>>::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(BULK_EVIDENCE_IDS_PER_QUERY) {
        let sql = format!("SELECT {id_column},scheme,notation FROM {table} WHERE {id_column} IN ({}) ORDER BY {id_column},scheme,notation", sql_placeholders(chunk.len()));
        let mut statement = connection.prepare(&sql).map_err(error)?;
        let rows = statement.query_map(params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))).map_err(error)?;
        for row in rows {
            let (owner_id, scheme, notation) = row.map_err(error)?;
            let scheme = match scheme {
                LCC_SCHEME => ClassificationScheme::LibraryOfCongress,
                _ => continue,
            };
            classifications.entry(owner_id).or_default().push((scheme, notation));
        }
    }
    Ok(classifications)
}

/// Resolves classification evidence for one edition being indexed.
fn resolve_edition_classifications(
    connection: &Connection, candidate: &EditionCandidate, authors: &[String], edition_classifications: &HashMap<i64, Vec<(ClassificationScheme, String)>>, work_classifications: &HashMap<i64, Vec<(ClassificationScheme, String)>>,
    has_title_author_codes: bool,
) -> Result<Option<Vec<Classification>>, MetadataError> {
    let has_author_keys = authors.iter().any(|author| !author_match_keys(author).is_empty());
    if has_author_keys && metadata_contract::matching::matching_author_count(authors, authors) == 0 {
        return Ok(None);
    }
    let mut classifications = BTreeMap::<(ClassificationScheme, String), ClassificationSource>::new();
    for (scheme, notation) in edition_classifications.get(&candidate.edition_id).into_iter().flatten() {
        classifications.insert((*scheme, notation.clone()), ClassificationSource::ExactEdition);
    }
    if let Some(work_id) = candidate.work_id {
        let exact_schemes = classifications.keys().map(|(scheme, _)| *scheme).collect::<BTreeSet<_>>();
        for (scheme, notation) in work_classifications.get(&work_id).into_iter().flatten() {
            if !exact_schemes.contains(scheme) {
                classifications.entry((*scheme, notation.clone())).or_insert(ClassificationSource::Work);
            }
        }
    }
    if classifications.is_empty() && has_title_author_codes {
        for (scheme, notation) in load_title_author_classifications(connection, authors, candidate)? {
            classifications.insert((scheme, notation), ClassificationSource::ExactEdition);
        }
    }
    let classifications = metadata_contract::filter_classification_outliers(classifications.into_iter().map(|((scheme, notation), source)| Classification { evidence: Vec::new(), scheme, notation, source }), classification_similarity_keys);
    // Candidate generation requires a usable LCC code. DDC-only records
    // cannot supply subjects through the ISBN-to-LCC enrichment path.
    if !classifications.iter().any(|c| c.scheme == ClassificationScheme::LibraryOfCongress && metadata_contract::classification_consensus_notation(c.scheme, &c.notation).is_some()) {
        return Ok(None);
    }
    Ok(Some(classifications))
}

fn load_title_author_classifications(connection: &Connection, authors: &[String], candidate: &EditionCandidate) -> Result<Vec<(ClassificationScheme, String)>, MetadataError> {
    if authors.iter().all(|author| author_match_keys(author).is_empty()) {
        return Ok(Vec::new());
    }
    let sql = "SELECT normalized_author,scheme,notation FROM title_author_classification WHERE normalized_title=?1 AND normalized_subtitle=?2 ORDER BY scheme,notation LIMIT 129";
    let values: [rusqlite::types::Value; 2] = [
        metadata_contract::matching::bibliographic_title_match_key(candidate.edition_title.as_deref().or(candidate.work_title.as_deref()).unwrap_or_default()).into(),
        candidate.subtitle.as_deref().map(|value| value.trim().to_lowercase()).unwrap_or_default().into(),
    ];
    let mut statement = connection.prepare_cached(sql).map_err(error)?;
    let rows = statement.query_map(params_from_iter(values), |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
    if rows.len() > 128 {
        return Ok(Vec::new());
    }
    let mut classifications = BTreeSet::new();
    for row in rows {
        let (author, scheme, notation) = row;
        if !authors.iter().any(|local| metadata_contract::matching::author_names_compatible(local, &author)) {
            continue;
        }
        let scheme = match scheme {
            1 => ClassificationScheme::DeweyDecimal,
            LCC_SCHEME => ClassificationScheme::LibraryOfCongress,
            _ => continue,
        };
        classifications.insert((scheme, notation));
    }
    Ok(classifications.into_iter().collect())
}

/// Resolve edition evidence once. Candidate ranking still uses the caller's facts.
pub(crate) fn build_candidates(source: &Connection, output: &Connection) -> Result<(), MetadataError> {
    build_candidates_for(source, output, None)
}

pub(crate) fn build_candidates_for(source: &Connection, output: &Connection, selected: Option<&BTreeSet<i64>>) -> Result<(), MetadataError> {
    let has_title_author_codes = source.query_row("SELECT EXISTS(SELECT 1 FROM title_author_classification LIMIT 1)", [], |r| r.get::<_, bool>(0)).map_err(error)?;
    select_candidate_editions(source, output, selected, has_title_author_codes)?;
    let mut after = i64::MIN;
    let mut scanned = 0_u64;
    let mut candidates = 0_u64;
    let started = std::time::Instant::now();
    let mut work_cache = WorkEvidenceCache::default();
    loop {
        let selected = {
            let mut stmt = output.prepare_cached("SELECT edition_id FROM candidate_edition WHERE edition_id>?1 ORDER BY edition_id LIMIT 4096").map_err(error)?;
            let ids = stmt.query_map([after], |r| r.get::<_, i64>(0)).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
            ids
        };
        if selected.is_empty() {
            break;
        }
        after = *selected.last().unwrap();
        scanned += selected.len() as u64;
        let sql = format!("SELECT edition_id,work_id FROM edition WHERE edition_id IN ({}) ORDER BY edition_id", sql_placeholders(selected.len()));
        let mut stmt = source.prepare_cached(&sql).map_err(error)?;
        let ids = stmt.query_map(params_from_iter(&selected), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
        let editions = ids.iter().map(|r| r.0).collect::<BTreeSet<_>>();
        let isbns = load_edition_isbns(source, &editions)?;
        let ec = load_classifications_by_ids(source, "edition_classification", "edition_id", &editions)?;
        let fallback_works = ids.iter().filter(|(id, _)| !ec.contains_key(id)).filter_map(|r| r.1).collect::<BTreeSet<_>>();
        let wc: HashMap<i64, Vec<(ClassificationScheme, String)>> =
            load_work_codes_cached(source, &fallback_works, &mut work_cache)?.into_iter().filter(|(_, codes)| !codes.is_empty()).map(|(work, codes)| (work, codes.into_iter().collect())).collect();
        let has_source_codes = |id: &i64, work: Option<i64>| ec.contains_key(id) || work.is_some_and(|work| wc.contains_key(&work));
        let initial_fallbacks = ids.iter().filter(|(id, work)| !has_source_codes(id, *work)).filter_map(|(id, _)| isbns.get(id).copied()).collect::<BTreeSet<_>>();
        let mut answer_classifications = load_answer_classifications(output, &initial_fallbacks)?;
        // With no title-author authority rows, an edition without either source
        // classifications or a classified answer cannot become a candidate.
        let eligible = ids.iter().filter(|(id, work)| has_source_codes(id, *work) || has_title_author_codes || isbns.get(id).is_some_and(|isbn| answer_classifications.contains_key(isbn))).map(|r| r.0).collect::<BTreeSet<_>>();
        let eligible_works = ids.iter().filter(|r| eligible.contains(&r.0)).filter_map(|r| r.1).collect::<BTreeSet<_>>();
        let ea = load_edition_authors(source, &eligible)?;
        let wa = load_work_authors(source, &eligible_works)?;
        let publishers = load_edition_publishers(source, &eligible)?;
        let mut rows = Vec::new();
        for chunk in eligible.iter().copied().collect::<Vec<_>>().chunks(BULK_EVIDENCE_IDS_PER_QUERY) {
            let sql = format!("SELECT e.edition_id,e.work_id,b.title,w.title,b.book_year,COALESCE(s.subtitle,CASE WHEN b.normalized_title=w.normalized_title OR b.title IS NULL THEN ws.subtitle END),s.normalized_title,ws.normalized_title,b.normalized_title,w.normalized_title FROM edition e LEFT JOIN edition_bibliography b USING(edition_id) LEFT JOIN work_bibliography w USING(work_id) LEFT JOIN edition_subtitle s USING(edition_id) LEFT JOIN work_subtitle ws USING(work_id) WHERE e.edition_id IN ({}) ORDER BY e.edition_id", sql_placeholders(chunk.len()));
            let mut query = source.prepare_cached(&sql).map_err(error)?;
            rows.extend(
                query
                    .query_map(params_from_iter(chunk), |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, Option<i64>>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, Option<String>>(3)?,
                            r.get::<_, Option<i32>>(4)?,
                            r.get::<_, Option<String>>(5)?,
                            [r.get::<_, Option<String>>(6)?, r.get(7)?, r.get(8)?, r.get(9)?],
                        ))
                    })
                    .map_err(error)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(error)?,
            );
        }
        let mut planned = Vec::with_capacity(rows.len());
        for (id, work, title, work_title, year, subtitle, keys) in rows {
            let authors = ea.get(&id).filter(|a| !a.is_empty()).or_else(|| work.and_then(|w| wa.get(&w))).cloned().unwrap_or_default();
            let candidate = EditionCandidate { edition_id: id, work_id: work, edition_title: title, subtitle, work_title, book_year: year };
            let matched = if has_title_author_codes || has_source_codes(&id, work) {
                resolve_edition_classifications(source, &candidate, &authors, &ec, &wc, has_title_author_codes)?.map(|classifications| EditionIdentityMatch {
                    subtitle: candidate.subtitle.clone(),
                    canonical_isbn13: isbns.get(&id).map(ToString::to_string).unwrap_or_default(),
                    open_library_edition_id: if id > 0 { format!("OL{id}M") } else { String::new() },
                    open_library_work_id: work.map(|work_id| format!("OL{work_id}W")),
                    title: candidate.edition_title.clone().or_else(|| candidate.work_title.clone()).unwrap_or_default(),
                    work_title: candidate.work_title.clone(),
                    title_match: EditionTitleMatch::Edition,
                    authors: authors.clone(),
                    publishers: publishers.get(&id).into_iter().flatten().map(|(name, _)| name.clone()).collect(),
                    book_year: year,
                    matched_publisher: false,
                    matched_book_year: false,
                    classifications,
                })
            } else {
                None
            };
            planned.push((id, work, candidate, authors, keys, matched));
        }
        // Source codes that failed validation may still have a classified ISBN
        // answer. Read those fallbacks together rather than deserializing every answer.
        let remaining_fallbacks = planned.iter().filter(|row| row.5.is_none()).filter_map(|row| isbns.get(&row.0).copied()).filter(|isbn| !initial_fallbacks.contains(isbn)).collect::<BTreeSet<_>>();
        answer_classifications.extend(load_answer_classifications(output, &remaining_fallbacks)?);
        for (id, work, candidate, authors, keys, mut matched) in planned {
            if matched.is_none() {
                if let Some(isbn) = isbns.get(&id) {
                    if let Some(classifications) = answer_classifications.get(isbn) {
                        matched = Some(EditionIdentityMatch {
                            canonical_isbn13: isbn.to_string(),
                            open_library_edition_id: if id > 0 { format!("OL{id}M") } else { String::new() },
                            open_library_work_id: candidate.work_id.map(|w| format!("OL{w}W")),
                            title: candidate.edition_title.clone().or_else(|| candidate.work_title.clone()).unwrap_or_default(),
                            subtitle: candidate.subtitle,
                            work_title: candidate.work_title,
                            authors,
                            publishers: publishers.get(&id).into_iter().flatten().map(|p| p.0.clone()).collect(),
                            book_year: candidate.book_year,
                            title_match: EditionTitleMatch::Edition,
                            matched_publisher: false,
                            matched_book_year: false,
                            classifications: classifications.clone(),
                        });
                    }
                }
            }
            let Some(m) = matched else { continue };
            candidates += 1;
            output
                .prepare_cached("INSERT INTO edition_candidate VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)")
                .map_err(error)?
                .execute(rusqlite::params![
                    id,
                    isbns.get(&id),
                    work,
                    m.title,
                    m.subtitle,
                    m.work_title,
                    serde_json::to_string(&m.authors).map_err(error)?,
                    serde_json::to_string(&m.publishers).map_err(error)?,
                    m.book_year,
                    serde_json::to_vec(&m.classifications).map_err(error)?
                ])
                .map_err(error)?;
            // Edition/subtitle/work keys often coincide. Combine their flags
            // before writing rather than updating the same search row four times.
            let mut search_keys = BTreeMap::<String, (i64, i64)>::new();
            for (index, key) in keys.into_iter().enumerate() {
                if let Some(key) = key.filter(|k| !k.is_empty()) {
                    let edition = i64::from(index == 0 || index == 2);
                    let work = i64::from(index == 1 || index == 3);
                    search_keys.entry(key).and_modify(|flags| { flags.0 = flags.0.max(edition); flags.1 = flags.1.max(work); }).or_insert((edition, work));
                }
            }
            for (key, (edition, work)) in search_keys {
                output
                        .prepare_cached("INSERT OR REPLACE INTO edition_title_key VALUES(?1,?2,MAX(?3,COALESCE((SELECT edition_match FROM edition_title_key WHERE key=?1 AND edition_id=?2),0)),MAX(?4,COALESCE((SELECT work_match FROM edition_title_key WHERE key=?1 AND edition_id=?2),0)))")
                        .map_err(error)?
                        .execute(rusqlite::params![key, id, edition, work])
                        .map_err(error)?;
            }
        }
        if scanned % 100_000 < 4096 {
            tracing::info!(edition_rows = scanned, candidates, elapsed_seconds = started.elapsed().as_secs_f64(), "building edition search index");
        }
    }
    output.execute_batch("DROP TABLE candidate_edition").map_err(error)?;
    Ok(())
}

fn load_answer_classifications(output: &Connection, isbns: &BTreeSet<Isbn13>) -> Result<HashMap<Isbn13, Vec<Classification>>, MetadataError> {
    let mut answers = HashMap::new();
    for chunk in isbns.iter().copied().collect::<Vec<_>>().chunks(BULK_EVIDENCE_IDS_PER_QUERY) {
        let sql = format!("SELECT isbn13,classifications FROM answer_classification WHERE isbn13 IN ({}) ORDER BY isbn13", sql_placeholders(chunk.len()));
        let mut query = output.prepare_cached(&sql).map_err(error)?;
        for row in query.query_map(params_from_iter(chunk), |r| Ok((r.get::<_, Isbn13>(0)?, r.get::<_, Vec<u8>>(1)?))).map_err(error)? {
            let (isbn, json) = row.map_err(error)?;
            answers.insert(isbn, serde_json::from_slice(&json).map_err(error)?);
        }
        // Legacy immutable bases have no compact summary table. Read only the
        // requested fallback answers; never materialize the whole base.
        for isbn in chunk.iter().filter(|isbn| !answers.contains_key(isbn)).copied().collect::<Vec<_>>() {
            let json: Option<Vec<u8>> = output.query_row("SELECT answer FROM isbn_answer WHERE isbn13=?1", [isbn], |r| r.get(0)).optional().map_err(error)?;
            if let Some(json) = json {
                let answer: crate::schema::StoredAnswer = serde_json::from_slice(&json).map_err(error)?;
                if answer.result.matches.len() == 1 && crate::build::has_lcc(&answer.result) {
                    answers.insert(isbn, answer.result.matches.into_iter().next().unwrap().classifications);
                }
            }
        }
    }
    Ok(answers)
}

/// Enumerate evidence owners through their indexes before loading edition facts.
/// The fallback uses the lowest ISBN, exactly as candidate resolution does.
fn select_candidate_editions(source: &Connection, output: &Connection, selected: Option<&BTreeSet<i64>>, has_title_author_codes: bool) -> Result<(), MetadataError> {
    output.execute_batch("CREATE TEMP TABLE candidate_edition(edition_id INTEGER PRIMARY KEY)").map_err(error)?;
    if let Some(selected) = selected {
        let mut insert = output.prepare_cached("INSERT INTO candidate_edition VALUES(?1)").map_err(error)?;
        for id in selected {
            insert.execute([id]).map_err(error)?;
        }
        return Ok(());
    }
    let path = source.path().ok_or_else(|| MetadataError("candidate selection requires a file-backed source".into()))?;
    let mut uri = reqwest::Url::from_file_path(std::fs::canonicalize(path).map_err(error)?).map_err(|_| MetadataError("invalid source path".into()))?;
    uri.set_query(Some("mode=ro"));
    output.execute("ATTACH DATABASE ?1 AS candidate_source", [uri.as_str()]).map_err(error)?;
    let result = if has_title_author_codes {
        output.execute_batch(
            "INSERT INTO candidate_edition SELECT edition_id FROM candidate_source.edition WHERE EXISTS(SELECT 1 FROM candidate_source.edition_isbn i WHERE i.edition_id=edition.edition_id);
             INSERT OR IGNORE INTO candidate_edition SELECT edition_id FROM candidate_source.edition_classification WHERE scheme=2;
             INSERT OR IGNORE INTO candidate_edition SELECT e.edition_id FROM
                 (SELECT work_id FROM candidate_source.work_classification WHERE scheme=2 UNION SELECT work_id FROM candidate_source.edition_work_classification WHERE scheme=2) c
                 JOIN candidate_source.edition e USING(work_id);",
        )
    } else {
        output.execute_batch(
            "INSERT OR IGNORE INTO candidate_edition SELECT edition_id FROM candidate_source.edition_classification WHERE scheme=2;
             INSERT OR IGNORE INTO candidate_edition SELECT e.edition_id FROM
                 (SELECT work_id FROM candidate_source.work_classification WHERE scheme=2 UNION SELECT work_id FROM candidate_source.edition_work_classification WHERE scheme=2) c
                 JOIN candidate_source.edition e USING(work_id);
             INSERT OR IGNORE INTO candidate_edition SELECT i.edition_id FROM answer_classification a JOIN candidate_source.edition_isbn i USING(isbn13)
                 WHERE a.isbn13=(SELECT MIN(isbn13) FROM candidate_source.edition_isbn first WHERE first.edition_id=i.edition_id);",
        )
    };
    // Keep the attachment until the enclosing output transaction commits.
    result.map_err(error)?;
    let count: i64 = output.query_row("SELECT count(*) FROM candidate_edition", [], |r| r.get(0)).map_err(error)?;
    tracing::info!(eligible_editions = count, "selected edition evidence owners");
    Ok(())
}
