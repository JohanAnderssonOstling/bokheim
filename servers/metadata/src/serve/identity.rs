//! Bounded independent authority lookup after work resolution abstains.
use crate::*;
use metadata_contract::matching::{matching_author_count_with_title, NormalizedTitle};
use metadata_contract::AuthoritySubjectMatch;

pub(super) fn validate_query(query: &EditionIdentityQuery) -> Result<(), MetadataError> {
    if query.query_id.is_empty() || query.query_id.len() > 256 || query.title.len() > 4096 || query.authors.len() > 32 || query.publishers.len() > 32 {
        return Err(MetadataError("edition query contains too many or oversized fields".into()));
    }
    Ok(())
}

/// An unresolved work never starts an edition-title search. Independent
/// LC/Wikidata records may still supply subjects without choosing an edition.
pub(super) fn resolve_authority(connection: &Connection, query: &EditionIdentityQuery) -> Result<EditionIdentityResult, MetadataError> {
    let status = if NormalizedTitle::new(&query.title).full.is_empty() { EditionIdentityStatus::InsufficientMetadata } else { EditionIdentityStatus::NoMatch };
    let mut result = identity_result(&query.query_id, status, None, vec![], false);
    result.authority_subjects = authority(connection, query)?;
    Ok(result)
}

pub(super) fn identity_result(id: &str, status: EditionIdentityStatus, matched: Option<EditionIdentityMatch>, candidates: Vec<EditionIdentityMatch>, candidates_truncated: bool) -> EditionIdentityResult {
    EditionIdentityResult { query_id: id.into(), status, matched, candidates, candidates_truncated, authority_subjects: None, sibling_subjects: None }
}

pub(super) fn authority(connection: &Connection, query: &EditionIdentityQuery) -> Result<Option<metadata_contract::AuthoritySubjectMatch>, MetadataError> {
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
