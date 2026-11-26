//! Bounded subject evidence from already verified works, without a title search.
use super::{
    identity::identity_result,
    sibling_subjects,
    subject_identity::{self, Candidate},
};
use crate::build::identity::{load_classifications_by_ids, load_edition_authors, load_edition_isbns, load_edition_publishers};
use crate::build::work_edition_index::EditionEvidence;
use crate::*;
use metadata_contract::matching::{bibliographic_match_key, matching_author_count};
use metadata_contract::subject_consensus::titles_compatible;
use metadata_contract::{classification_consensus_notation, AuthoritySubjectMatch, SiblingSubjectSample};
use rusqlite::params_from_iter;

const MAX_CLASSIFIED_EDITIONS: usize = 32;
const MAX_EXAMINED_EDITIONS: usize = 128;
const PAGE_SIZE: usize = 16;
const MAX_EDITION_REFERENCES: usize = 512;
const MAX_PUBLICATION_CANDIDATES: usize = 128;
const MIN_SAMPLED_OBSERVATIONS: usize = 8;
pub(super) const MAX_WORKS: usize = 128;

type Works<'a> = BTreeMap<i64, &'a AuthoritySubjectMatch>;

fn work_map(records: &[AuthoritySubjectMatch]) -> Works<'_> {
    records.iter().filter_map(|record| open_library_id(&record.record_ids[0], 'W').map(|id| (id, record))).collect()
}

fn load_candidates(c: &Connection, query: &EditionIdentityQuery, works: &Works<'_>, ids: &BTreeSet<i64>) -> Result<BTreeMap<i64, Candidate>, MetadataError> {
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let authors = load_edition_authors(c, ids)?;
    let publishers = if query.publishers.is_empty() { HashMap::new() } else { load_edition_publishers(c, ids)? };
    let isbns = load_edition_isbns(c, ids)?;
    let codes = load_classifications_by_ids(c, "edition_classification", "edition_id", ids)?;
    let sql = format!(
        "SELECT e.edition_id,e.work_id,b.title,COALESCE(s.subtitle,CASE WHEN b.title IS NULL OR b.normalized_title=w.normalized_title THEN ws.subtitle END),b.book_year FROM edition e LEFT JOIN edition_bibliography b USING(edition_id) LEFT JOIN edition_subtitle s USING(edition_id) LEFT JOIN work_bibliography w USING(work_id) LEFT JOIN work_subtitle ws USING(work_id) WHERE e.edition_id IN ({}) ORDER BY e.edition_id",
        sql_placeholders(ids.len())
    );
    let mut statement = c.prepare_cached(&sql).map_err(error)?;
    let rows = statement.query_map(params_from_iter(ids), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, Option<String>>(3)?, r.get::<_, Option<i32>>(4)?))).map_err(error)?;
    let mut evidence = Vec::new();
    for row in rows {
        let (id, work_id, title, subtitle, year) = row.map_err(error)?;
        evidence.push(EditionEvidence {
            edition_id: id,
            work_id,
            title,
            subtitle,
            book_year: year,
            authors: authors.get(&id).cloned().unwrap_or_default(),
            isbn13: isbns.get(&id).map(ToString::to_string).unwrap_or_default(),
            codes: codes.get(&id).into_iter().flatten().map(|(_, notation)| notation.clone()).collect(),
        });
    }
    Ok(evaluate_candidates(query, works, evidence, publishers))
}

fn load_indexed_candidates(index: &Connection, query: &EditionIdentityQuery, works: &Works<'_>, references: &[(i64, i64)]) -> Result<BTreeMap<i64, Candidate>, MetadataError> {
    let mut statement = index.prepare_cached("SELECT evidence FROM edition_lcc WHERE work_id=?1 AND edition_id=?2").map_err(error)?;
    let mut evidence = Vec::with_capacity(references.len());
    for &(work_id, edition_id) in references {
        let raw: Vec<u8> = statement.query_row(params![work_id, edition_id], |r| r.get(0)).map_err(error)?;
        let record: EditionEvidence = serde_json::from_slice(&raw).map_err(error)?;
        if record.work_id != work_id || record.edition_id != edition_id {
            return Err(MetadataError("work edition evidence identity mismatch".into()));
        }
        evidence.push(record);
    }
    // Publishers only select exact editions, a separate path. They neither
    // qualify nor weight observations in subject consensus.
    Ok(evaluate_candidates(query, works, evidence, HashMap::new()))
}

fn evaluate_candidates(query: &EditionIdentityQuery, works: &Works<'_>, evidence: Vec<EditionEvidence>, publishers: HashMap<i64, Vec<(String, String)>>) -> BTreeMap<i64, Candidate> {
    let publisher_keys = query.publishers.iter().map(|value| bibliographic_match_key(value)).collect::<BTreeSet<_>>();
    let verified = works.values().map(|work| work.record_ids[0].clone()).collect::<BTreeSet<_>>();
    let mut result = BTreeMap::new();
    for record in evidence {
        let id = record.edition_id;
        let Some(work) = works.get(&record.work_id) else { continue };
        let matched_publisher = publishers.get(&id).into_iter().flatten().any(|(_, key)| publisher_keys.contains(key));
        let matched_book_year = query.book_year.is_some() && query.book_year == record.book_year;
        let matched = EditionIdentityMatch {
            canonical_isbn13: record.isbn13,
            open_library_edition_id: format!("OL{id}M"),
            open_library_work_id: Some(work.record_ids[0].clone()),
            title: record.title.unwrap_or_else(|| work.main_title.clone()),
            subtitle: record.subtitle,
            work_title: Some(work.title.clone()),
            authors: if record.authors.is_empty() { work.authors.clone() } else { record.authors },
            publishers: publishers.get(&id).into_iter().flatten().map(|(name, _)| name.clone()).collect(),
            book_year: record.book_year,
            title_match: EditionTitleMatch::Work,
            matched_publisher,
            matched_book_year,
            classifications: record
                .codes
                .into_iter()
                .filter(|notation| classification_consensus_notation(ClassificationScheme::LibraryOfCongress, notation).is_some())
                .map(|notation| Classification { scheme: ClassificationScheme::LibraryOfCongress, notation, source: ClassificationSource::ExactEdition, evidence: vec![] })
                .collect(),
        };
        if !subject_identity::title_agrees(query, &matched, &verified) {
            continue;
        }
        let count = subject_identity::author_count(query, &matched, &verified);
        if count == 0 {
            continue;
        }
        result.insert(id, (query.title.chars().count(), count, usize::from(matched_publisher) + usize::from(matched_book_year), matched));
    }
    result
}

/// Older imports without a derived index must also have a finite probe budget.
fn references(c: &Connection, works: &Works<'_>) -> Result<(Vec<(i64, i64)>, bool), MetadataError> {
    let sql = format!("SELECT work_id,edition_id FROM edition WHERE edition_id>0 AND work_id IN ({}) ORDER BY work_id,edition_id LIMIT {}", sql_placeholders(works.len()), MAX_EDITION_REFERENCES + 1);
    let mut statement = c.prepare_cached(&sql).map_err(error)?;
    let mut rows = statement.query_map(params_from_iter(works.keys()), |r| Ok((r.get(0)?, r.get(1)?))).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
    let truncated = rows.len() > MAX_EDITION_REFERENCES;
    rows.truncate(MAX_EDITION_REFERENCES);
    Ok((rows, truncated))
}

/// Publisher/year identification is separate from the consensus sample.
/// A year index bounds publisher reads; incomplete candidate sets abstain.
pub(super) fn exact(c: &Connection, index: Option<&Connection>, query: &EditionIdentityQuery, records: &[AuthoritySubjectMatch], answers: &Pool<SqliteConnectionManager>) -> Result<Option<EditionIdentityResult>, MetadataError> {
    let Some(year) = query.book_year else { return Ok(None) };
    let publishers = query.publishers.iter().map(|value| bibliographic_match_key(value)).filter(|value| !value.is_empty()).collect::<BTreeSet<_>>();
    if publishers.is_empty() {
        return Ok(None);
    }
    let works = work_map(records);
    let year_ids = if let Some(index) = index {
        let sql = format!("SELECT edition_id FROM edition_year WHERE work_id IN ({}) AND book_year=? ORDER BY work_id,book_year,edition_id LIMIT {}", sql_placeholders(works.len()), MAX_PUBLICATION_CANDIDATES + 1);
        let values = works.keys().copied().chain([i64::from(year)]).collect::<Vec<_>>();
        let mut statement = index.prepare_cached(&sql).map_err(error)?;
        let ids = statement.query_map(params_from_iter(values), |r| r.get::<_, i64>(0)).map_err(error)?.collect::<Result<BTreeSet<_>, _>>().map_err(error)?;
        ids
    } else {
        let (references, truncated) = references(c, &works)?;
        if truncated || references.is_empty() {
            return Ok(None);
        }
        let sql = format!("SELECT edition_id FROM edition_bibliography WHERE edition_id IN ({}) AND book_year=? LIMIT {}", sql_placeholders(references.len()), MAX_PUBLICATION_CANDIDATES + 1);
        let values = references.into_iter().map(|(_, id)| id).chain([i64::from(year)]).collect::<Vec<_>>();
        let mut statement = c.prepare_cached(&sql).map_err(error)?;
        let ids = statement.query_map(params_from_iter(values), |r| r.get::<_, i64>(0)).map_err(error)?.collect::<Result<BTreeSet<_>, _>>().map_err(error)?;
        ids
    };
    if year_ids.is_empty() || year_ids.len() > MAX_PUBLICATION_CANDIDATES {
        return Ok(None);
    }
    let names = load_edition_publishers(c, &year_ids)?;
    let ids = year_ids.into_iter().filter(|id| names.get(id).is_some_and(|names| names.iter().any(|(_, key)| publishers.contains(key)))).collect::<BTreeSet<_>>();
    if ids.len() != 1 {
        return Ok(None);
    }
    let Some((_, _, _, mut matched)) = load_candidates(c, query, &works, &ids)?.into_values().next() else { return Ok(None) };
    if matched.classifications.is_empty() {
        // Archived provider evidence may exist only in the prepared answer
        // candidate. One point read preserves its exact-edition priority.
        let answers = answers.get().map_err(error)?;
        let raw: Option<Vec<u8>> = answers.query_row("SELECT classifications FROM edition_candidate WHERE edition_id=?1", [*ids.first().unwrap()], |r| r.get(0)).optional().map_err(error)?;
        if let Some(raw) = raw {
            let codes: Vec<Classification> = serde_json::from_slice(&raw).map_err(error)?;
            matched.classifications = codes
                .into_iter()
                .filter(|code| code.source == ClassificationSource::ExactEdition && code.scheme == ClassificationScheme::LibraryOfCongress && classification_consensus_notation(code.scheme, &code.notation).is_some())
                .collect();
        }
    }
    if !matched.matched_publisher || !matched.matched_book_year || !titles_compatible(&query.title, &matched.full_title()) || matching_author_count(&query.authors, &matched.authors) == 0 || matched.classifications.is_empty() {
        return Ok(None);
    }
    Ok(Some(identity_result(&query.query_id, EditionIdentityStatus::Matched, Some(matched), vec![], false)))
}

pub(super) fn resolve(c: &Connection, index: Option<&Connection>, query: &EditionIdentityQuery, records: &[AuthoritySubjectMatch]) -> Result<EditionIdentityResult, MetadataError> {
    let works = work_map(records);
    let (mut ids, mut truncated) = if let Some(index) = index {
        let sql = format!("SELECT work_id,edition_id FROM edition_lcc WHERE work_id IN ({}) ORDER BY work_id,edition_id LIMIT {}", sql_placeholders(works.len()), MAX_EXAMINED_EDITIONS + 1);
        let mut statement = index.prepare_cached(&sql).map_err(error)?;
        let rows = statement.query_map(params_from_iter(works.keys()), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
        (rows, false)
    } else {
        let (references, truncated) = references(c, &works)?;
        if references.is_empty() {
            return Ok(identity_result(&query.query_id, EditionIdentityStatus::NoMatch, None, vec![], false));
        }
        let sql = format!("SELECT DISTINCT edition_id FROM edition_classification WHERE scheme=2 AND edition_id IN ({})", sql_placeholders(references.len()));
        let mut statement = c.prepare_cached(&sql).map_err(error)?;
        let classified = statement.query_map(params_from_iter(references.iter().map(|(_, id)| id)), |r| r.get::<_, i64>(0)).map_err(error)?.collect::<Result<BTreeSet<_>, _>>().map_err(error)?;
        (references.into_iter().filter(|(_, id)| classified.contains(id)).collect(), truncated)
    };
    truncated |= ids.len() > MAX_EXAMINED_EDITIONS;
    ids.truncate(MAX_EXAMINED_EDITIONS);
    let mut examined = 0;
    let mut candidates = Vec::new();
    let mut observations = BTreeSet::new();
    'pages: for page in ids.chunks(PAGE_SIZE) {
        let mut loaded = if let Some(index) = index {
            load_indexed_candidates(index, query, &works, page)?
        } else {
            let page_ids = page.iter().map(|(_, id)| *id).collect::<BTreeSet<_>>();
            load_candidates(c, query, &works, &page_ids)?
        };
        let preceding = examined;
        examined += page.len();
        for (offset, (_, id)) in page.iter().enumerate() {
            let Some(candidate) = loaded.remove(id) else { continue };
            if candidate.3.classifications.is_empty() {
                continue;
            }
            let identity = if candidate.3.canonical_isbn13.is_empty() { candidate.3.open_library_edition_id.clone() } else { candidate.3.canonical_isbn13.clone() };
            observations.insert(identity);
            candidates.push(candidate);
            if observations.len() == MAX_CLASSIFIED_EDITIONS {
                truncated |= preceding + offset + 1 < ids.len();
                break 'pages;
            }
        }
    }
    let mut consensus = sibling_subjects::consensus(query, &candidates);
    // A partial input containing only a few eligible records cannot establish
    // sample support. Complete small works retain the existing voting policy.
    if truncated && consensus.classified_editions < MIN_SAMPLED_OBSERVATIONS {
        consensus.subjects.clear();
        consensus.used_highest_support_fallback = false;
    }
    consensus.sample = Some(SiblingSubjectSample { examined_editions: examined, classified_edition_limit: MAX_CLASSIFIED_EDITIONS, truncated });
    tracing::debug!(works = works.len(), examined_editions = examined, classified_editions = consensus.classified_editions, truncated, "resolved work edition evidence");
    let mut result = identity_result(&query.query_id, EditionIdentityStatus::NoMatch, None, vec![], truncated);
    result.sibling_subjects = Some(consensus);
    Ok(result)
}
