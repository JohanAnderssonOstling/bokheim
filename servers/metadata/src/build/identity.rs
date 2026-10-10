use crate::Isbn13;
use crate::{
    classification_similarity_keys, error, sql_placeholders, BTreeMap, BTreeSet, Classification, ClassificationScheme, ClassificationSource, Connection, EditionIdentityMatch, EditionTitleMatch, HashMap, MetadataError,
    BULK_EVIDENCE_IDS_PER_QUERY, LCC_SCHEME,
};
use crate::build::import::core::author_match_keys;

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
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).map_err(error)?;
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
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))).map_err(error)?;
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
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Isbn13>(1)?))).map_err(error)?;
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
        let rows = statement.query_map(rusqlite::params_from_iter(chunk.iter()), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))).map_err(error)?;
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
    sibling_classifications: &HashMap<i64, Vec<(ClassificationScheme, String)>>,
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
        let mut work_schemes = BTreeSet::new();
        for (scheme, notation) in work_classifications.get(&work_id).into_iter().flatten() {
            if !exact_schemes.contains(scheme) {
                work_schemes.insert(*scheme);
                classifications.entry((*scheme, notation.clone())).or_insert(ClassificationSource::Work);
            }
        }
        for (scheme, notation) in sibling_classifications.get(&work_id).into_iter().flatten() {
            if !exact_schemes.contains(scheme) && !work_schemes.contains(scheme) {
                classifications.entry((*scheme, notation.clone())).or_insert(ClassificationSource::Work);
            }
        }
    }
    if classifications.is_empty() {
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
    let mut statement = connection.prepare(sql).map_err(error)?;
    let rows = statement.query_map(rusqlite::params_from_iter(values), |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?))).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
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
    let mut after = i64::MIN;
    loop {
        let mut stmt = source.prepare("SELECT e.edition_id,e.work_id,b.title,w.title,b.book_year,COALESCE(s.subtitle,CASE WHEN b.normalized_title=w.normalized_title OR b.title IS NULL THEN ws.subtitle END),s.normalized_title,ws.normalized_title,b.normalized_title,w.normalized_title FROM edition e LEFT JOIN edition_bibliography b USING(edition_id) LEFT JOIN work_bibliography w USING(work_id) LEFT JOIN edition_subtitle s USING(edition_id) LEFT JOIN work_subtitle ws USING(work_id) WHERE e.edition_id>?1 ORDER BY e.edition_id LIMIT 1024").map_err(error)?;
        let rows = stmt
            .query_map([after], |r| {
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
            .map_err(error)?;
        if rows.is_empty() {
            break;
        }
        after = rows.last().unwrap().0;
        let editions = rows.iter().map(|r| r.0).collect::<BTreeSet<_>>();
        let works = rows.iter().filter_map(|r| r.1).collect::<BTreeSet<_>>();
        let ea = load_edition_authors(source, &editions)?;
        let wa = load_work_authors(source, &works)?;
        let publishers = load_edition_publishers(source, &editions)?;
        let isbns = load_edition_isbns(source, &editions)?;
        let ec = load_classifications_by_ids(source, "edition_classification", "edition_id", &editions)?;
        let wc = load_classifications_by_ids(source, "work_classification", "work_id", &works)?;
        let sc = load_classifications_by_ids(source, "edition_work_classification", "work_id", &works)?;
        for (id, work, title, work_title, year, subtitle, keys) in rows {
            let authors = ea.get(&id).filter(|a| !a.is_empty()).or_else(|| work.and_then(|w| wa.get(&w))).cloned().unwrap_or_default();
            let candidate = EditionCandidate { edition_id: id, work_id: work, edition_title: title, subtitle, work_title, book_year: year };
            let mut matched = if let Some(isbn) = isbns.get(&id) {
                resolve_edition_classifications(source, &candidate, &authors, &ec, &wc, &sc)?.map(|classifications| EditionIdentityMatch {
                    subtitle: candidate.subtitle.clone(),
                    canonical_isbn13: isbn.to_string(),
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
            if matched.is_none() {
                if let Some(isbn) = isbns.get(&id) {
                    let raw: Vec<u8> = output.query_row("SELECT answer FROM isbn_answer WHERE isbn13=?1", [isbn], |r| r.get(0)).map_err(error)?;
                    let stored: crate::schema::StoredAnswer = serde_json::from_slice(&raw).map_err(error)?;
                    if stored.result.matches.len() == 1 && crate::build::has_lcc(&stored.result) {
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
                            classifications: stored.result.matches[0].classifications.clone(),
                        });
                    }
                }
            }
            let Some(m) = matched else { continue };
            output
                .execute(
                    "INSERT INTO edition_candidate VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                    rusqlite::params![
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
                    ],
                )
                .map_err(error)?;
            for (index, key) in keys.into_iter().enumerate() {
                if let Some(key) = key.filter(|k| !k.is_empty()) {
                    let edition = i64::from(index == 0 || index == 2);
                    let work = i64::from(index == 1 || index == 3);
                    output
                        .execute(
                            "INSERT INTO edition_title_key VALUES(?1,?2,?3,?4) ON CONFLICT(key,edition_id) DO UPDATE SET edition_match=MAX(edition_match,excluded.edition_match),work_match=MAX(work_match,excluded.work_match)",
                            rusqlite::params![key, id, edition, work],
                        )
                        .map_err(error)?;
                }
            }
        }
    }
    Ok(())
}
