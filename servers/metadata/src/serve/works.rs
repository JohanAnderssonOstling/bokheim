//! Resolve work identities directly, with indexed edition-description fallback.
use super::{
    identity::identity_result,
    subject_identity::{authors, conflicting_credits},
    work_editions,
};
use crate::build::edition_descriptions::EditionDescriptions;
use crate::build::identity::{load_classifications_by_ids, load_names_by_ids};
use crate::build::work_search_index::{self, MAX_AUTHOR_KEYS};
use crate::*;
use metadata_contract::authority_subjects::classification_record;
use metadata_contract::matching::{matching_author_count_with_title, NormalizedTitle};
use metadata_contract::subject_consensus::titles_compatible;
use metadata_contract::works::{WorkIdentityMatch, WorkIdentityQuery, WorkIdentityResult, WorkIdentityStatus, MAX_WORK_QUERIES};
use metadata_contract::{authority_subjects::OPEN_LIBRARY_WORK_PROVIDER, AuthoritySubjectMatch, ClassificationEvidence, ClassificationMethod, ClassificationProvider};
use rusqlite::{params_from_iter, types::Value};

const REVIEW_LIMIT: usize = 64;
const WORK_LIMIT: usize = 4096;

#[derive(Clone)]
pub struct WorkLookup {
    pool: Pool<SqliteConnectionManager>,
    search_index: Pool<SqliteConnectionManager>,
    edition_index: Option<Pool<SqliteConnectionManager>>,
    edition_descriptions: Option<EditionDescriptions>,
    pub(crate) dump_date: String,
}

impl WorkLookup {
    pub fn open(path: &Path) -> Result<Self, MetadataError> {
        let path = path.canonicalize().map_err(error)?;
        let edition_index = crate::build::work_edition_index::open_index(&path)?;
        let pool = crate::build::openlibrary::open_description_pool(&path)?;
        let c = pool.get().map_err(error)?;
        let (version, dump_date): (i64, String) = c.query_row("SELECT schema_version,dump_date FROM snapshot WHERE singleton=1", [], |r| Ok((r.get(0)?, r.get(1)?))).map_err(error)?;
        let builder: i64 = c.query_row("SELECT value FROM metadata_metric WHERE name='builder_schema_version'", [], |r| r.get(0)).map_err(error)?;
        if version != SCHEMA_VERSION || ![9, 10, 11].contains(&builder) {
            return Err(MetadataError("work lookup requires an imported schema 9, 10 or 11 snapshot".into()));
        }
        c.prepare("SELECT b.work_id,b.title,s.subtitle,b.normalized_title,s.normalized_title FROM work_bibliography b LEFT JOIN work_subtitle s USING(work_id) LIMIT 0").map_err(error)?;
        for sql in ["SELECT w.work_id,a.name FROM work_author w JOIN author a USING(author_id) LIMIT 0", "SELECT work_id,scheme,notation FROM work_classification LIMIT 0"] {
            c.prepare(sql).map_err(error)?;
        }
        drop(c);
        let search_index = work_search_index::open_index(&path)?;
        let edition_descriptions = EditionDescriptions::open(&path, &pool)?;
        Ok(Self { pool, search_index, edition_index, edition_descriptions, dump_date })
    }

    pub fn resolve(&self, queries: Vec<WorkIdentityQuery>) -> Result<Vec<WorkIdentityResult>, MetadataError> {
        if queries.is_empty() || queries.len() > MAX_WORK_QUERIES {
            return Err(MetadataError(format!("between 1 and {MAX_WORK_QUERIES} work queries are required")));
        }
        let c = self.pool.get().map_err(error)?;
        queries.into_iter().map(|query| self.lookup(&c, &query).map(|(result, _)| result)).collect()
    }

    /// Duplicate work IDs may share one description. Missing descriptions do
    /// not conflict, but conflicting credits or nonempty texts must abstain.
    pub(super) fn description(&self, queries: &[EditionIdentityQuery], isbns: &[String]) -> Result<Option<String>, MetadataError> {
        let c = self.pool.get().map_err(error)?;
        let mut works = BTreeSet::new();
        for query in queries {
            let query = WorkIdentityQuery { query_id: query.query_id.clone(), title: query.title.clone(), authors: query.authors.clone() };
            let (result, matches) = self.lookup(&c, &query)?;
            if result.candidates_truncated || result.status == WorkIdentityStatus::Ambiguous && matches.is_empty() {
                break;
            }
            if matches.is_empty() {
                continue;
            }
            works = matches.iter().flat_map(|matched| &matched.record_ids).filter_map(|id| open_library_id(id, 'W')).collect();
            let descriptions = load_embedded_work_descriptions(&c, &works)?.into_values().map(|text| text.trim().to_owned()).filter(|text| !text.is_empty()).collect::<BTreeSet<_>>();
            // Do not select a different reading to hide conflicting descriptions
            // among works already verified against the primary title and author.
            if !descriptions.is_empty() {
                return Ok((descriptions.len() == 1).then(|| descriptions.into_iter().next().expect("one description")));
            }
            break;
        }
        drop(c);
        self.edition_descriptions.as_ref().map(|editions| editions.description(isbns, &works)).transpose().map(Option::flatten)
    }

    pub(super) fn require_separate(&self, demand: &Path) -> Result<(), MetadataError> {
        crate::snapshot::require_separate(&*self.pool.get().map_err(error)?, demand, "demand")?;
        crate::snapshot::require_separate(&*self.search_index.get().map_err(error)?, demand, "demand")?;
        if let Some(index) = &self.edition_index {
            crate::snapshot::require_separate(&*index.get().map_err(error)?, demand, "demand")?;
        }
        if let Some(descriptions) = &self.edition_descriptions {
            descriptions.require_separate(demand)?;
        }
        Ok(())
    }

    pub(super) fn editions_indexed(&self) -> bool {
        self.edition_index.is_some()
    }

    fn lookup(&self, c: &Connection, query: &WorkIdentityQuery) -> Result<(WorkIdentityResult, Vec<AuthoritySubjectMatch>), MetadataError> {
        if query.query_id.is_empty() || query.query_id.len() > 256 || query.title.len() > 4096 || query.authors.len() > 32 || query.authors.iter().any(|a| a.len() > 4096) {
            return Err(MetadataError("work query contains too many or oversized fields".into()));
        }
        let title = NormalizedTitle::new(&query.title);
        let local_authors = authors(&query.authors);
        let mut result = WorkIdentityResult { query_id: query.query_id.clone(), status: WorkIdentityStatus::NoMatch, matched: None, candidates: vec![], candidates_truncated: false };
        if title.full.is_empty() || metadata_contract::matching::matching_author_count(&local_authors, &local_authors) == 0 {
            result.status = WorkIdentityStatus::InsufficientMetadata;
            return Ok((result, vec![]));
        }
        let author_keys = work_search_index::author_keys(&local_authors);
        if author_keys.len() > MAX_AUTHOR_KEYS {
            result.status = WorkIdentityStatus::Ambiguous;
            result.candidates_truncated = true;
            return Ok((result, vec![]));
        }
        if author_keys.is_empty() {
            result.status = WorkIdentityStatus::InsufficientMetadata;
            return Ok((result, vec![]));
        }
        let index = self.search_index.get().map_err(error)?;
        let sql = format!("SELECT DISTINCT work_id FROM work_search WHERE key IN ({}) ORDER BY work_id LIMIT {}", sql_placeholders(author_keys.len()), WORK_LIMIT + 1);
        let mut statement = index.prepare_cached(&sql).map_err(error)?;
        for (key, _) in title.candidate_keys(MAX_TITLE_SUBSTRINGS_PER_QUERY) {
            let search_keys = author_keys.iter().map(|author| work_search_index::search_key(&key, author)).collect::<Vec<_>>();
            let ids = statement.query_map(params_from_iter(search_keys.iter().map(|key| key.as_slice())), |r| r.get::<_, i64>(0)).map_err(error)?.collect::<Result<BTreeSet<_>, _>>().map_err(error)?;
            if ids.len() > WORK_LIMIT {
                result.status = WorkIdentityStatus::Ambiguous;
                result.candidates_truncated = true;
                return Ok((result, vec![]));
            }
            if ids.is_empty() {
                continue;
            }
            // Hashes only retrieve candidates. Recheck the original title key
            // before the existing full-title and author compatibility checks.
            let sql = format!(
                "SELECT b.work_id,b.title,s.subtitle FROM work_bibliography b LEFT JOIN work_subtitle s USING(work_id) WHERE b.work_id IN ({}) AND (b.normalized_title=? OR s.normalized_title=?) ORDER BY b.work_id",
                sql_placeholders(ids.len())
            );
            let mut values = ids.iter().copied().map(Value::Integer).collect::<Vec<_>>();
            values.extend([Value::Text(key.clone()), Value::Text(key)]);
            let mut metadata = c.prepare_cached(&sql).map_err(error)?;
            let rows = metadata.query_map(params_from_iter(values), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?))).map_err(error)?.collect::<Result<Vec<_>, _>>().map_err(error)?;
            tracing::debug!(candidate_works = rows.len(), "indexed title and author candidates");
            let mut matches = Vec::new();
            let mut best = 0;
            let ids = rows.iter().map(|r| r.0).collect::<BTreeSet<_>>();
            let credits = load_names_by_ids(c, &ids, "work_author", "work_id")?;
            for (id, main_title, subtitle) in rows {
                let title = subtitle.as_deref().filter(|s| !s.trim().is_empty()).map_or_else(|| main_title.clone(), |s| format!("{main_title}: {s}"));
                let record = AuthoritySubjectMatch { main_title, title, authors: credits.get(&id).cloned().unwrap_or_default(), provider_id: OPEN_LIBRARY_WORK_PROVIDER.into(), record_ids: vec![format!("OL{id}W")], classifications: vec![] };
                if !titles_compatible(&query.title, &record.title) {
                    continue;
                }
                let count = matching_author_count_with_title(&local_authors, &authors(&record.authors), &query.title, &record.title);
                if count == 0 || count < best {
                    continue;
                }
                if count > best {
                    matches.clear();
                    best = count;
                }
                matches.push(record);
            }
            if matches.is_empty() {
                continue;
            }
            let conflict = conflicting_credits(&query.authors, matches.iter().map(|record| record.authors.as_slice()));
            let identity = |record: &AuthoritySubjectMatch| WorkIdentityMatch { open_library_work_id: record.record_ids[0].clone(), title: record.title.clone(), authors: record.authors.clone() };
            if matches.len() == 1 && !conflict {
                result.status = WorkIdentityStatus::Matched;
                result.matched = Some(identity(&matches[0]));
            } else {
                result.status = WorkIdentityStatus::Ambiguous;
                result.candidates_truncated = matches.len() > REVIEW_LIMIT;
                result.candidates = matches.iter().take(REVIEW_LIMIT).map(identity).collect();
            }
            // Explicitly conflicting full names must not be hidden by pooling
            // classifications, even when the local credit contains an initial.
            return Ok((result, if conflict { vec![] } else { matches }));
        }
        Ok((result, vec![]))
    }

    /// Carry verified work IDs into the fallback instead of repeating a broad
    /// title/author search. Samples supply subjects, never an edition identity.
    pub(super) fn resolve_subjects(&self, query: &EditionIdentityQuery, answers: &Pool<SqliteConnectionManager>) -> Result<Option<EditionIdentityResult>, MetadataError> {
        let work_query = WorkIdentityQuery { query_id: query.query_id.clone(), title: query.title.clone(), authors: query.authors.clone() };
        let c = self.pool.get().map_err(error)?;
        let (result, matches) = self.lookup(&c, &work_query)?;
        if matches.is_empty() {
            return Ok(None);
        }
        if matches.len() > work_editions::MAX_WORKS {
            let mut result = identity_result(&query.query_id, EditionIdentityStatus::Ambiguous, None, vec![], true);
            result.sibling_subjects = Some(Default::default());
            return Ok(Some(result));
        }
        let index = self.edition_index.as_ref().map(|pool| pool.get().map_err(error)).transpose()?;
        let index = index.as_deref();
        if let Some(exact) = work_editions::exact(&c, index, query, &matches, answers)? {
            return Ok(Some(exact));
        }
        if result.status == WorkIdentityStatus::Matched {
            if let Some(work) = Self::declared_subjects(&c, query, &matches)? {
                let mut result = identity_result(&query.query_id, EditionIdentityStatus::NoMatch, None, vec![], false);
                result.authority_subjects = Some(work);
                return Ok(Some(result));
            }
        }
        let mut result = work_editions::resolve(&c, index, query, &matches)?;
        if !result.sibling_subjects.as_ref().is_some_and(|consensus| !consensus.subjects.is_empty()) {
            // Retain independent LC/Wikidata recovery without reopening the
            // broad edition search when the bounded work evidence abstains.
            let answers = answers.get().map_err(error)?;
            result.authority_subjects = super::identity::authority(&answers, query)?;
        }
        Ok(Some(result))
    }

    fn declared_subjects(c: &Connection, query: &EditionIdentityQuery, matches: &[AuthoritySubjectMatch]) -> Result<Option<AuthoritySubjectMatch>, MetadataError> {
        let Some(mut matched) = matches.first().cloned() else { return Ok(None) };
        let work = matched.record_ids[0].clone();
        let id = open_library_id(&work, 'W').ok_or_else(|| MetadataError("invalid work lookup identity".into()))?;
        let codes = load_classifications_by_ids(c, "work_classification", "work_id", &BTreeSet::from([id]))?;
        matched.classifications = codes
            .get(&id)
            .into_iter()
            .flatten()
            .map(|(scheme, notation)| Classification {
                scheme: *scheme,
                notation: notation.clone(),
                source: ClassificationSource::Work,
                evidence: vec![ClassificationEvidence {
                    method: ClassificationMethod::TitleAuthor { provider: ClassificationProvider::OpenLibraryWork, record_id: work.clone() },
                    isbn13: String::new(),
                    open_library_work_id: work.clone(),
                    open_library_edition_id: String::new(),
                }],
            })
            .collect();
        Ok(classification_record(query, &matched).map(|record| {
            matched.classifications = record.classifications;
            matched
        }))
    }
}
