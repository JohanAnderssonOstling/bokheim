use crate::*;
use super::subject_identity::{self, Candidate};
use rusqlite::{Result as SqliteResult, Row};
use metadata_contract::classification_consensus_notation;
use metadata_contract::authority_subjects::{classification_record, OPEN_LIBRARY_WORK_PROVIDER};
use metadata_contract::AuthoritySubjectMatch;
use metadata_contract::matching::{author_match_keys, bibliographic_match_key, matching_author_count, matching_author_count_with_title, title_qualifiers, NormalizedTitle};

// Bound the candidate side before joining the merged base/update views. SQLite
// otherwise materializes every edition in the view for a single title lookup.
pub(crate) const CANDIDATE_QUERY: &str = "WITH title_candidates AS MATERIALIZED (SELECT edition_id,edition_match,work_match FROM edition_title_key WHERE key=?1 ORDER BY edition_id LIMIT 4097)
SELECT c.isbn13,c.edition_id,c.work_id,c.title,c.subtitle,c.work_title,c.authors,c.publishers,c.book_year,c.classifications,k.edition_match,k.work_match
FROM title_candidates k JOIN (SELECT * FROM edition_candidate WHERE edition_id IN (SELECT edition_id FROM title_candidates)) c USING(edition_id) ORDER BY k.edition_id LIMIT 4097";
const WORK_CANDIDATE_QUERY: &str = "WITH title_candidates AS MATERIALIZED (SELECT edition_id,edition_match,work_match FROM edition_title_key WHERE key=?1 AND work_match=1)
SELECT c.isbn13,c.edition_id,c.work_id,c.title,c.subtitle,c.work_title,c.authors,c.publishers,c.book_year,c.classifications,k.edition_match,k.work_match
FROM title_candidates k JOIN (SELECT * FROM edition_candidate WHERE edition_id IN (SELECT edition_id FROM title_candidates)) c USING(edition_id) WHERE c.work_id=?2 ORDER BY c.edition_id LIMIT 4097";

pub(crate) fn rank_matches(query: &EditionIdentityQuery, author_keys: &BTreeSet<String>, mut editions: Vec<(usize, usize, usize, EditionIdentityMatch)>) -> EditionIdentityResult {
    if editions.len() > 1 {
        let best_specificity = editions.iter().map(|(specificity, _, _, _)| *specificity).max().unwrap_or(0);
        editions.retain(|(specificity, _, _, _)| *specificity == best_specificity);
    }
    if editions.len() > 1 {
        let best_authors = editions.iter().map(|(_, count, _, _)| *count).max().unwrap_or(0);
        editions.retain(|(_, count, _, _)| *count == best_authors);
    }
    if editions.len() > 1 && !author_keys.is_empty() {
        let best_score = editions.iter().map(|(_, _, score, _)| *score).max().unwrap_or(0);
        if best_score > 0 {
            editions.retain(|(_, _, score, _)| *score == best_score);
        }
    }
    let uses_omitted_names = editions.iter().any(|(_, count, _, matched)| matching_author_count(&query.authors, &matched.authors) < *count);
    // A short credit cannot choose between explicitly different full names.
    // Check before classification voting or work deduplication can hide one.
    let author_conflict = uses_omitted_names && conflicting_author_names(query, &editions);
    if !author_conflict {
        super::candidate_subjects::corroborate(query, &mut editions);
    }
    if editions.len() > 1 && !author_conflict && !uses_omitted_names {
        let mut seen_works = BTreeSet::new();
        // Editions of one work can supply different classifications. Keep a
        // representative of each code set so the first indexed edition does
        // not hide supported subjects from the remaining editions.
        editions.retain(|(_, _, _, matched)| matched.open_library_work_id.as_ref().is_none_or(|work_id| {
            let codes = matched.classifications.iter().filter_map(|classification| {
                classification_consensus_notation(classification.scheme, &classification.notation).map(|notation| (classification.scheme, notation))
            }).collect::<BTreeSet<_>>();
            seen_works.insert((work_id.clone(), codes))
        }));
    }
    // Duplicate records sharing one ISBN and identical codes can collapse.
    // Different ISBNs remain unresolved even when their classifications agree;
    // callers can recover work subjects without inventing an edition identity.
    if editions.len() > 1 && !author_conflict && editions.iter().all(|(_, matched_author, _, _)| *matched_author > 0) {
        let classification_codes = |matched: &EditionIdentityMatch| {
            matched.classifications.iter().map(|classification| (classification.scheme, classification.notation.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_uppercase).collect::<String>())).collect::<BTreeSet<_>>()
        };
        let first_codes = classification_codes(&editions[0].3);
        if !first_codes.is_empty() && !editions[0].3.canonical_isbn13.is_empty()
            && editions.iter().skip(1).all(|(_, _, _, matched)| matched.canonical_isbn13 == editions[0].3.canonical_isbn13 && matched.book_year == editions[0].3.book_year && classification_codes(matched) == first_codes)
        {
            editions.truncate(1);
        }
    }
    // Title agreement identifies candidates, but it is not enough to attach
    // an external edition automatically: generic titles such as "Complete
    // Works" routinely collide across unrelated authors. Publisher and year
    // may rank editions of the same work, while at least one author must agree
    // before the remaining candidate is safe to accept without review.
    // Short/full credits support work subjects, but do not establish an exact
    // edition ISBN. Keep a sole relaxed match available for subject recovery.
    let automatic_match = editions.len() == 1 && matching_author_count(&query.authors, &editions[0].3.authors) > 0;
    let mut strong = editions.into_iter().map(|(_, _, _, matched)| matched).collect::<Vec<_>>();
    match strong.len() {
        0 => identity_result(&query.query_id, EditionIdentityStatus::NoMatch, None, Vec::new(), false),
        1 if automatic_match => identity_result(&query.query_id, EditionIdentityStatus::Matched, strong.pop(), Vec::new(), false),
        _ => {
            let candidates_truncated = strong.len() > MAX_EDITION_IDENTITY_REVIEW_CANDIDATES;
            strong.truncate(MAX_EDITION_IDENTITY_REVIEW_CANDIDATES);
            identity_result(&query.query_id, EditionIdentityStatus::Ambiguous, None, strong, candidates_truncated)
        }
    }
}

fn conflicting_author_names(query: &EditionIdentityQuery, editions: &[Candidate]) -> bool {
    subject_identity::conflicting_authors(query, &editions.iter().map(|(_, _, _, candidate)| candidate).collect::<Vec<_>>())
}

type CandidateRow = (Option<Isbn13>, i64, Option<i64>, String, Option<String>, Option<String>, String, String, Option<i32>, Vec<u8>, bool, bool);

fn candidate_row(row: &Row<'_>) -> SqliteResult<CandidateRow> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?))
}

fn candidate(query: &EditionIdentityQuery, publisher_keys: &BTreeSet<String>, row: CandidateRow) -> Result<Option<EditionIdentityMatch>, MetadataError> {
    let (isbn, id, work, title, subtitle, work_title, authors, publishers, year, codes, edition_match, work_match) = row;
    let authors: Vec<String> = serde_json::from_str(&authors).map_err(error)?;
    let publishers: Vec<String> = serde_json::from_str(&publishers).map_err(error)?;
    let matched_publisher = publishers.iter().any(|publisher| publisher_keys.contains(&bibliographic_match_key(publisher)));
    let matched = EditionIdentityMatch {
        canonical_isbn13: isbn.map(|isbn| isbn.to_string()).unwrap_or_default(),
        open_library_edition_id: if id > 0 { format!("OL{id}M") } else { String::new() },
        open_library_work_id: work.map(|work| format!("OL{work}W")),
        title, subtitle, work_title, authors, publishers, book_year: year,
        title_match: match (edition_match, work_match) { (true, true) => EditionTitleMatch::Both, (true, false) => EditionTitleMatch::Edition, _ => EditionTitleMatch::Work },
        matched_publisher, matched_book_year: query.book_year.is_some() && query.book_year == year,
        classifications: serde_json::from_slice(&codes).map_err(error)?,
    };
    Ok((title_qualifiers(&matched.full_title()) == title_qualifiers(&query.title)).then_some(matched))
}

/// Only subject evidence is expanded; these rows cannot choose an edition ISBN.
fn sibling_candidates(connection: &Connection, query: &EditionIdentityQuery, publisher_keys: &BTreeSet<String>, seeds: &[Candidate], works: &BTreeSet<String>) -> Result<Option<Vec<Candidate>>, MetadataError> {
    if works.len() > 128 { return Ok(None); }
    let mut candidates = seeds.to_vec();
    let mut seen = seeds.iter().map(|(_, _, _, candidate)| candidate.open_library_edition_id.clone()).collect::<BTreeSet<_>>();
    // Every indexed edition carries its work's title key, even when its own
    // title is translated or different. Use that existing index: older full
    // snapshots have no index on edition_candidate.work_id.
    let mut keys = BTreeMap::<String, BTreeSet<String>>::new();
    for (_, _, _, seed) in seeds {
        let Some(work) = seed.open_library_work_id.as_ref().filter(|work| works.contains(*work)) else { continue };
        if let Some(title) = &seed.work_title {
            keys.entry(work.clone()).or_default().extend(NormalizedTitle::new(title).lookup_keys());
        }
    }
    let mut statement = connection.prepare_cached(WORK_CANDIDATE_QUERY).map_err(error)?;
    let specificity = seeds.iter().map(|candidate| candidate.0).max().unwrap_or(0);
    let mut visited = BTreeSet::new();
    for (work, keys) in keys {
        let Some(id) = work.strip_prefix("OL").and_then(|work| work.strip_suffix('W')).and_then(|work| work.parse::<i64>().ok()) else { continue };
        for key in keys {
            let rows = statement.query_map(params![key, id], candidate_row).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
            if rows.len() > MAX_REVERSE_TITLE_CANDIDATES { return Ok(None); }
            for row in rows {
                // Count the complete work search before author/volume filters.
                // A truncated input cannot establish a support denominator.
                visited.insert(row.1);
                if visited.len() > MAX_REVERSE_TITLE_CANDIDATES { return Ok(None); }
                let Some(matched) = candidate(query, publisher_keys, row)? else { continue };
                if !seen.insert(matched.open_library_edition_id.clone()) { continue; }
                let count = subject_identity::author_count(query, &matched, works);
                if count == 0 { continue; }
                let score = usize::from(matched.matched_publisher) + usize::from(matched.matched_book_year);
                candidates.push((specificity, count, score, matched));
            }
        }
    }
    Ok(Some(candidates))
}

pub(crate) fn resolve(connection: &Connection, queries: Vec<EditionIdentityQuery>) -> Result<Vec<EditionIdentityResult>, MetadataError> {
    queries.into_iter().map(|query| {
        if query.query_id.is_empty() || query.query_id.len() > 256 || query.title.len() > 4096 || query.authors.len() > 32 || query.publishers.len() > 32 {
            return Err(MetadataError("edition query contains too many or oversized fields".into()));
        }
        let title = NormalizedTitle::new(&query.title);
        let author_keys = query.authors.iter().flat_map(|author| author_match_keys(author)).collect::<BTreeSet<_>>();
        let publisher_keys = query.publishers.iter().map(|publisher| bibliographic_match_key(publisher)).collect::<BTreeSet<_>>();
        let mut result = identity_result(&query.query_id, if title.full.is_empty() { EditionIdentityStatus::InsufficientMetadata } else { EditionIdentityStatus::NoMatch }, None, vec![], false);
        let mut fallback = None;
        let mut statement = connection.prepare_cached(CANDIDATE_QUERY).map_err(error)?;
        for (key, specificity) in title.candidate_keys(MAX_TITLE_SUBSTRINGS_PER_QUERY) {
            let rows = statement.query_map([key], candidate_row).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
            if rows.len() > MAX_REVERSE_TITLE_CANDIDATES {
                return Ok(identity_result(&query.query_id, EditionIdentityStatus::Ambiguous, None, vec![], true));
            }
            let mut matches = Vec::new();
            for row in rows {
                let Some(matched) = candidate(&query, &publisher_keys, row)? else { continue };
                let count = subject_identity::author_count(&query, &matched, &BTreeSet::new());
                if !author_keys.is_empty() && count == 0 { continue; }
                let score = usize::from(matched.matched_publisher) + usize::from(matched.matched_book_year);
                matches.push((specificity, count, score, matched));
            }
            if let Some(work) = declared_work(connection, &query, &matches)? {
                result = identity_result(&query.query_id, EditionIdentityStatus::NoMatch, None, vec![], false);
                result.authority_subjects = Some(work);
                return Ok(result);
            }
            let works = subject_identity::verified_works(&query, &matches);
            let expanded = sibling_candidates(connection, &query, &publisher_keys, &matches, &works)?;
            let consensus = expanded.as_ref().map(|candidates| super::sibling_subjects::consensus(&query, candidates)).unwrap_or_default();
            result = rank_matches(&query, &author_keys, matches);
            result.candidates_truncated |= expanded.is_none();
            let supported = !consensus.subjects.is_empty();
            result.sibling_subjects = Some(consensus);
            let exact = result.matched.as_ref().is_some_and(|matched| matched.matched_publisher && matched.matched_book_year
                && matched.classifications.iter().any(|code| code.source == ClassificationSource::ExactEdition && code.scheme == ClassificationScheme::LibraryOfCongress && classification_consensus_notation(code.scheme, &code.notation).is_some()));
            // A bare edition match is not a completed subject lookup. Once all
            // compatible siblings of its verified work were read, the evidence
            // is exhausted even if no subject meets the support threshold.
            if supported || exact || result.candidates_truncated || !works.is_empty() { return Ok(result); }
            if fallback.is_none() && result.status != EditionIdentityStatus::NoMatch { fallback = Some(result.clone()); }
        }
        if let Some(previous) = fallback { result = previous; }
        if result.authority_subjects.is_none() && !result.sibling_subjects.as_ref().is_some_and(|consensus| !consensus.subjects.is_empty()) {
            result.authority_subjects = authority(connection, &query)?;
        }
        Ok(result)
    }).collect()
}

/// Title/author agreement establishes a work before it establishes an edition.
/// Read only the work's own declared codes here; candidate edition codes are a fallback.
fn declared_work(connection: &Connection, query: &EditionIdentityQuery, editions: &[(usize, usize, usize, EditionIdentityMatch)]) -> Result<Option<AuthoritySubjectMatch>, MetadataError> {
    let Some(specificity) = editions.iter().map(|e| e.0).max() else { return Ok(None) };
    let best_authors = editions.iter().filter(|e| e.0 == specificity).map(|e| e.1).max().unwrap_or(0);
    if best_authors == 0 {
        return Ok(None);
    }
    let score = editions.iter().filter(|e| e.0 == specificity && e.1 == best_authors).map(|e| e.2).max().unwrap_or(0);
    let edition_ranked = editions.iter().filter(|e| e.0 == specificity && e.1 == best_authors && e.2 == score).map(|e| &e.3).collect::<Vec<_>>();
    // A year alone can be the EPUB creation year. Require publisher and year
    // before a title match can prefer a particular edition over the work.
    if edition_ranked.len() == 1 && edition_ranked[0].matched_publisher && edition_ranked[0].matched_book_year
        && edition_ranked[0].classifications.iter().any(|c| c.source == ClassificationSource::ExactEdition && c.scheme == ClassificationScheme::LibraryOfCongress && classification_consensus_notation(c.scheme, &c.notation).is_some())
    {
        return Ok(None);
    }
    let ranked = editions.iter().filter(|e| e.0 == specificity && e.1 == best_authors).cloned().collect::<Vec<_>>();
    if conflicting_author_names(query, &ranked) {
        return Ok(None);
    }
    let works = subject_identity::verified_works(query, &ranked);
    if works.len() > 128 {
        return Ok(None);
    }
    let mut accepted = Vec::new();
    for work in &works {
        let raw: Option<Vec<u8>> = connection
            .query_row("SELECT codes FROM authority_title WHERE key=?1 AND source=?2 AND control_number=?3", params![format!("openlibrary-work:{work}"), OPEN_LIBRARY_WORK_PROVIDER, work], |r| r.get(0))
            .optional().map_err(error)?;
        let Some(raw) = raw else { continue };
        let stored: AuthoritySubjectMatch = serde_json::from_slice(&raw).map_err(error)?;
        if stored.provider_id != OPEN_LIBRARY_WORK_PROVIDER || stored.record_ids != [work.as_str()] {
            return Err(MetadataError("declared work classification identity mismatch".into()));
        }
        // Edition titles supply aliases, including translated titles, for the
        // work lookup. Their own classifications do not enter this result.
        for (_, _, _, edition) in ranked.iter().filter(|e| e.3.open_library_work_id.as_deref() == Some(work.as_str())) {
            let mut matched = stored.clone();
            if !subject_identity::title_agrees(query, edition, &works) || subject_identity::author_count(query, edition, &works) == 0 { continue; }
            // The accepted edition/work link verifies this local title as a
            // work alias. Preserve the work's codes and control-number evidence.
            matched.title = query.title.clone();
            matched.main_title = query.title.clone();
            matched.authors = subject_identity::authors(&edition.authors);
            if classification_record(query, &matched).is_some() {
                accepted.push(matched);
                break;
            }
        }
    }
    let Some(mut result) = accepted.first().cloned() else { return Ok(None) };
    result.record_ids = accepted.iter().flat_map(|m| m.record_ids.clone()).collect();
    let mut codes = BTreeMap::<(ClassificationScheme, String), Classification>::new();
    for matched in accepted {
        for classification in matched.classifications {
            codes.entry((classification.scheme, classification.notation.clone())).and_modify(|old| old.evidence.extend(classification.evidence.clone())).or_insert(classification);
        }
    }
    result.classifications = codes.into_values().collect();
    Ok(Some(result))
}

fn identity_result(id: &str, status: EditionIdentityStatus, matched: Option<EditionIdentityMatch>, candidates: Vec<EditionIdentityMatch>, candidates_truncated: bool) -> EditionIdentityResult {
    EditionIdentityResult { query_id: id.into(), status, matched, candidates, candidates_truncated, authority_subjects: None, sibling_subjects: None }
}

fn authority(connection: &Connection, query: &EditionIdentityQuery) -> Result<Option<metadata_contract::AuthoritySubjectMatch>, MetadataError> {
    use metadata_contract::AuthoritySubjectMatch;
    if query.authors.is_empty() {
        return Ok(None);
    }
    let normalized = NormalizedTitle::new(&query.title);
    let mut stmt = connection.prepare_cached("SELECT codes FROM authority_title WHERE key=?1 AND source=?2 ORDER BY control_number LIMIT 129").map_err(error)?;
    for provider in ["library_of_congress", "wikidata"] {
        let mut groups = vec![];
        let mut wikidata = BTreeMap::new();
        for key in normalized.lookup_keys() {
            let rows = stmt.query_map(params![key, provider], |r| r.get::<_, Vec<u8>>(0)).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
            if rows.len() > 128 {
                return Ok(None);
            }
            let records = rows.into_iter().map(|raw| serde_json::from_slice::<AuthoritySubjectMatch>(&raw).map_err(error)).collect::<Result<Vec<_>, _>>()?;
            if provider == "wikidata" {
                for m in records {
                    let id = m.record_ids[0].strip_prefix('Q').and_then(|id| id.parse::<i64>().ok()).ok_or_else(|| MetadataError("invalid authority control number".into()))?;
                    wikidata.insert(-id, m);
                }
            } else {
                groups.push(records);
            }
        }
        if provider == "wikidata" {
            if wikidata.len() > 128 {
                return Ok(None);
            }
            groups.push(wikidata.into_values().collect());
        }
        for records in groups {
            let mut accepted: Vec<AuthoritySubjectMatch> = vec![];
            let mut best = 0;
            for m in records {
                let record = NormalizedTitle::new(&m.title);
                if normalized.qualifiers != record.qualifiers {
                    continue;
                }
                if provider == "library_of_congress" {
                    let main = NormalizedTitle::new(&m.main_title).full;
                    if normalized.full != record.full && normalized.full != normalized.main && record.full != main {
                        continue;
                    }
                } else if !normalized.matches(&record) && !normalized.matches(&NormalizedTitle::new(&m.main_title)) {
                    continue;
                }
                let count = matching_author_count_with_title(&query.authors, &m.authors, &query.title, &m.title);
                if count == 0 || count < best {
                    continue;
                }
                if count > best {
                    accepted.clear();
                    best = count;
                }
                accepted.push(m);
            }
            if accepted.is_empty() {
                continue;
            }
            let mut result = accepted[0].clone();
            result.record_ids = accepted.iter().flat_map(|m| m.record_ids.clone()).collect();
            let mut codes = BTreeMap::<(ClassificationScheme, String), Classification>::new();
            for m in accepted {
                for c in m.classifications {
                    codes.entry((c.scheme, c.notation.clone())).and_modify(|old| old.evidence.extend(c.evidence.clone())).or_insert(c);
                }
            }
            result.classifications = codes.into_values().collect();
            return Ok(Some(result));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod qualifier_tests {
    use super::*;

    #[test]
    fn a_matching_year_does_not_select_an_abridgment_hidden_in_the_subtitle() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(crate::schema::ANSWERS).unwrap();
        let classifications = serde_json::to_vec(&vec![Classification { notation: "PR4074".into(), scheme: ClassificationScheme::LibraryOfCongress, source: ClassificationSource::ExactEdition, evidence: vec![] }]).unwrap();
        for (id, subtitle, year) in [(1, Some("abridged from the original"), 2008), (2, None, 2007)] {
            db.execute("INSERT INTO edition_candidate VALUES(?1,9780306406157,?1,'Example book',?2,'Example book','[\"Known Author\"]','[]',?3,?4)", params![id, subtitle, year, classifications]).unwrap();
            db.execute("INSERT INTO edition_title_key VALUES('example book',?1,1,1)", [id]).unwrap();
        }
        let query = EditionIdentityQuery { query_id: "example".into(), title: "Example book".into(), authors: vec!["Known Author".into()], author_surname: None, publishers: vec![], book_year: Some(2008) };
        let result = resolve(&db, vec![query]).unwrap().pop().unwrap();
        assert_eq!(result.status, EditionIdentityStatus::Matched);
        assert_eq!(result.matched.unwrap().open_library_edition_id, "OL2M");
    }
}
