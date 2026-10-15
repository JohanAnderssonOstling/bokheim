//! Batch the indexed evidence reads; classification rules remain identical to lookup_one.
use super::enrichment::sql_placeholders;
use super::loc_dump::merge_lcc;
use super::openlibrary::authority_edition_key;
use super::work_classification::SiblingLccEvidence;
use crate::{error, Classification, ClassificationScheme, ClassificationSource, Isbn13, IsbnClassificationResult, LookupStatus, MetadataError, WorkClassificationMatch, LCC_SCHEME};
use rusqlite::{
    params_from_iter,
    types::{FromSql, ToSql},
    Connection,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

const QUERY_IDS: usize = 500;
pub(super) type Codes = BTreeSet<(ClassificationScheme, String)>;

/// Owned by a single build over immutable inputs; misses are cached too.
#[derive(Default)]
pub(crate) struct WorkEvidenceCache {
    classifications: EvidenceCache<Codes, 16_777_216>,
    pub(super) supplements: EvidenceCache<(Option<String>, Vec<Classification>), 16_777_216>,
}

#[derive(Default)]
pub(super) struct EvidenceCache<V, const LIMIT: usize> {
    rows: HashMap<i64, V>,
    order: VecDeque<(i64, usize)>,
    bytes: usize,
}

impl<V: Clone + Default, const LIMIT: usize> EvidenceCache<V, LIMIT> {
    pub(super) fn load(&mut self, ids: &BTreeSet<i64>, fetch: impl FnOnce(&BTreeSet<i64>) -> Result<BTreeMap<i64, V>, MetadataError>, weight: impl Fn(&V) -> usize) -> Result<BTreeMap<i64, V>, MetadataError> {
        let missing = ids.iter().filter(|id| !self.rows.contains_key(id)).copied().collect::<BTreeSet<_>>();
        let mut result = ids.iter().filter_map(|id| self.rows.get(id).map(|codes| (*id, codes.clone()))).collect::<BTreeMap<_, _>>();
        let fetched = if missing.is_empty() { BTreeMap::new() } else { fetch(&missing)? };
        for &id in ids {
            if result.contains_key(&id) {
                continue;
            }
            let codes = fetched.get(&id).cloned().unwrap_or_default();
            let bytes = 96 + weight(&codes);
            if bytes <= LIMIT {
                while self.bytes + bytes > LIMIT {
                    let (old, size) = self.order.pop_front().unwrap();
                    self.rows.remove(&old);
                    self.bytes -= size;
                }
                self.rows.insert(id, codes.clone());
                self.order.push_back((id, bytes));
                self.bytes += bytes;
            }
            result.insert(id, codes);
        }
        Ok(result)
    }
}

#[cfg(test)]
pub(crate) fn lookup_many(connection: &Connection, requested: Vec<String>) -> Result<Vec<IsbnClassificationResult>, MetadataError> {
    lookup_many_cached(connection, requested, &mut WorkEvidenceCache::default())
}

pub(crate) fn lookup_many_cached(connection: &Connection, requested: Vec<String>, cache: &mut WorkEvidenceCache) -> Result<Vec<IsbnClassificationResult>, MetadataError> {
    let parsed = requested.iter().map(|isbn| Isbn13::parse(isbn)).collect::<Vec<_>>();
    let isbns = parsed.iter().flatten().copied().collect::<BTreeSet<_>>();
    let mut editions = BTreeMap::<Isbn13, Vec<(i64, Option<i64>)>>::new();
    let mut edition_ids = BTreeSet::new();
    for chunk in isbns.iter().copied().collect::<Vec<_>>().chunks(QUERY_IDS) {
        let sql = format!("SELECT i.isbn13,e.edition_id,e.work_id FROM edition_isbn i JOIN edition e USING(edition_id) WHERE i.isbn13 IN ({}) ORDER BY i.isbn13,e.work_id,e.edition_id", sql_placeholders(chunk.len()));
        let mut q = connection.prepare_cached(&sql).map_err(error)?;
        for row in q.query_map(params_from_iter(chunk), |r| Ok((r.get::<_, Isbn13>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?))).map_err(error)? {
            let (isbn, edition, work) = row.map_err(error)?;
            editions.entry(isbn).or_default().push((edition, work));
            edition_ids.insert(edition);
        }
    }
    let exact = load_codes(connection, "edition_classification", "edition_id", &edition_ids, false)?;
    // Exact-edition LCC is first, then the work's own LCC, then sibling evidence.
    let fallback_works = editions
        .values()
        .flatten()
        .filter_map(|(edition, work)| {
            let work = (*work)?;
            exact.get(edition).is_none_or(BTreeSet::is_empty).then_some(work)
        })
        .collect::<BTreeSet<_>>();
    let works = load_work_codes_cached(connection, &fallback_works, cache)?;
    let direct = load_codes(connection, "isbn_classification", "isbn13", &isbns, true)?;
    let mut lc = BTreeMap::<Isbn13, BTreeSet<String>>::new();
    for chunk in isbns.iter().copied().collect::<Vec<_>>().chunks(QUERY_IDS) {
        let sql = format!("SELECT DISTINCT isbn13,notation FROM isbn_lcc WHERE isbn13 IN ({}) ORDER BY isbn13,notation", sql_placeholders(chunk.len()));
        let mut q = connection.prepare_cached(&sql).map_err(error)?;
        for row in q.query_map(params_from_iter(chunk), |r| Ok((r.get(0)?, r.get(1)?))).map_err(error)? {
            let (isbn, notation) = row.map_err(error)?;
            lc.entry(isbn).or_default().insert(notation);
        }
    }
    requested
        .into_iter()
        .zip(parsed)
        .map(|(requested_isbn, isbn)| {
            let Some(isbn) = isbn else {
                return Ok(IsbnClassificationResult { requested_isbn, canonical_isbn13: None, status: LookupStatus::InvalidIsbn, matches: vec![] });
            };
            let mut by_work = BTreeMap::<(Option<i64>, i64), Vec<i64>>::new();
            for &(edition, work) in editions.get(&isbn).into_iter().flatten() {
                by_work.entry((work, work.unwrap_or(edition))).or_default().push(edition);
            }
            let mut matches = Vec::with_capacity(by_work.len());
            for ((work_id, _), edition_ids) in by_work {
                let mut codes = BTreeMap::<(ClassificationScheme, String), ClassificationSource>::new();
                for edition in &edition_ids {
                    for code in exact.get(edition).into_iter().flatten() {
                        codes.insert(code.clone(), ClassificationSource::ExactEdition);
                    }
                }
                if let Some(work) = work_id {
                    let exact_schemes = codes.keys().map(|(scheme, _)| *scheme).collect::<BTreeSet<_>>();
                    for code in works.get(&work).into_iter().flatten() {
                        if !exact_schemes.contains(&code.0) {
                            codes.insert(code.clone(), ClassificationSource::Work);
                        }
                    }
                }
                matches.push(WorkClassificationMatch {
                    open_library_work_id: work_id.map(|work| format!("OL{work}W")),
                    exact_edition_ids: edition_ids.into_iter().map(authority_edition_key).collect(),
                    classifications: classifications(codes),
                });
            }
            if let Some(direct) = direct.get(&isbn).filter(|codes| !codes.is_empty()) {
                if matches.is_empty() {
                    matches.push(WorkClassificationMatch {
                        open_library_work_id: None,
                        exact_edition_ids: vec![],
                        classifications: direct.iter().map(|(scheme, notation)| Classification { evidence: vec![], scheme: *scheme, notation: notation.clone(), source: ClassificationSource::ExactEdition }).collect(),
                    });
                } else {
                    for matched in &mut matches {
                        let mut codes = matched.classifications.iter().map(|c| ((c.scheme, c.notation.clone()), c.source)).collect::<BTreeMap<_, _>>();
                        for code in direct {
                            codes.entry(code.clone()).or_insert(ClassificationSource::ExactEdition);
                        }
                        matched.classifications = classifications(codes);
                    }
                }
            }
            merge_lcc(&mut matches, lc.get(&isbn).cloned().unwrap_or_default());
            let status = match matches.len() {
                0 => LookupStatus::NoMatch,
                1 => LookupStatus::Matched,
                _ => LookupStatus::Ambiguous,
            };
            Ok(IsbnClassificationResult { requested_isbn, canonical_isbn13: Some(isbn.to_string()), status, matches })
        })
        .collect()
}

fn code_bytes(codes: &Codes) -> usize {
    codes.iter().map(|(_, notation)| notation.len() + 64).sum()
}

fn classifications(codes: BTreeMap<(ClassificationScheme, String), ClassificationSource>) -> Vec<Classification> {
    codes.into_iter().map(|((scheme, notation), source)| Classification { evidence: vec![], scheme, notation, source }).collect()
}

fn load_codes<K: Ord + Copy + ToSql + FromSql>(connection: &Connection, table: &str, column: &str, ids: &BTreeSet<K>, include_dewey: bool) -> Result<BTreeMap<K, Codes>, MetadataError> {
    let mut codes = BTreeMap::<K, Codes>::new();
    for chunk in ids.iter().copied().collect::<Vec<_>>().chunks(QUERY_IDS) {
        let sql = format!("SELECT {column},scheme,notation FROM {table} WHERE {column} IN ({}) ORDER BY {column},scheme,notation", sql_placeholders(chunk.len()));
        let mut q = connection.prepare_cached(&sql).map_err(error)?;
        for row in q.query_map(params_from_iter(chunk), |r| Ok((r.get::<_, K>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?))).map_err(error)? {
            let (id, scheme, notation) = row.map_err(error)?;
            let scheme = match scheme {
                LCC_SCHEME => ClassificationScheme::LibraryOfCongress,
                1 if include_dewey => ClassificationScheme::DeweyDecimal,
                _ => continue,
            };
            codes.entry(id).or_default().insert((scheme, notation));
        }
    }
    Ok(codes)
}

pub(super) fn load_work_codes_cached(connection: &Connection, works: &BTreeSet<i64>, cache: &mut WorkEvidenceCache) -> Result<BTreeMap<i64, Codes>, MetadataError> {
    cache.classifications.load(works, |missing| load_work_codes(connection, missing), code_bytes)
}

fn load_work_codes(connection: &Connection, works: &BTreeSet<i64>) -> Result<BTreeMap<i64, Codes>, MetadataError> {
    let mut imported = load_codes(connection, "work_classification", "work_id", works, false)?;
    for codes in imported.values_mut() {
        codes.retain(|(scheme, notation)| metadata_contract::classification_consensus_notation(*scheme, notation).is_some());
    }
    let mut support = BTreeMap::<i64, SiblingLccEvidence>::new();
    let fallback = works.iter().copied().filter(|work| imported.get(work).is_none_or(BTreeSet::is_empty)).collect::<Vec<_>>();
    for chunk in fallback.chunks(QUERY_IDS) {
        let sql =
            format!("SELECT e.work_id,c.edition_id,c.notation FROM edition e JOIN edition_classification c USING(edition_id) WHERE e.work_id IN ({}) AND c.scheme=2 ORDER BY e.work_id,c.edition_id,c.notation", sql_placeholders(chunk.len()));
        let mut q = connection.prepare_cached(&sql).map_err(error)?;
        for row in q.query_map(params_from_iter(chunk), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?))).map_err(error)? {
            let (work, edition, notation) = row.map_err(error)?;
            support.entry(work).or_default().add(edition, &notation);
        }
    }
    Ok(works
        .iter()
        .map(|work| {
            let declared = imported.remove(work).unwrap_or_default();
            let selected = if declared.is_empty() {
                support.remove(work).unwrap_or_default().select(BTreeSet::new()).into_iter().map(|code| (ClassificationScheme::LibraryOfCongress, code)).collect()
            } else {
                declared
            };
            (*work, selected)
        })
        .collect())
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    #[test]
    fn negative_work_evidence_is_reused_without_another_read() {
        let mut cache = EvidenceCache::<Codes, 8_388_608>::default();
        let ids = [1, 2].into_iter().collect();
        cache
            .load(
                &ids,
                |missing| {
                    assert_eq!(missing, &ids);
                    Ok(BTreeMap::new())
                },
                code_bytes,
            )
            .unwrap();
        let next = cache
            .load(
                &ids,
                |missing| {
                    assert!(missing.is_empty());
                    Ok(BTreeMap::new())
                },
                code_bytes,
            )
            .unwrap();
        assert_eq!(next.len(), 2);
        assert!(next.values().all(BTreeSet::is_empty));
    }

    #[test]
    fn eviction_during_a_batch_preserves_evidence_already_cached_for_that_batch() {
        let codes: Codes = [(ClassificationScheme::LibraryOfCongress, "Q".repeat(1024 * 1024))].into_iter().collect();
        let mut cache = EvidenceCache::<Codes, 8_388_608>::default();
        cache.load(&[100].into_iter().collect(), |_| Ok([(100, codes.clone())].into_iter().collect()), code_bytes).unwrap();
        let ids = (0..19).chain([100]).collect();
        let result = cache.load(&ids, |missing| Ok(missing.iter().map(|id| (*id, codes.clone())).collect()), code_bytes).unwrap();
        assert_eq!(result.len(), 20);
        assert_eq!(result[&100], codes);
        assert!(cache.bytes <= 8 * 1024 * 1024);
        assert!(!cache.rows.contains_key(&100));
    }
}
