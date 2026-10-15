//! Classification-only authority evidence, independent of an ISBN or OL identity.
use crate::{AuthoritySubjectMatch, ClassificationEvidence, ClassificationScheme, ClassificationSource, EditionIdentityQuery, RichWorkMetadataMatch};
pub const METHOD_PREFIX: &str = "library_of_congress_title_author:";
pub const OPEN_LIBRARY_WORK_PROVIDER: &str = "openlibrary_work";

pub fn classification_record(query: &EditionIdentityQuery, matched: &AuthoritySubjectMatch) -> Option<RichWorkMetadataMatch> {
    use crate::matching::{matching_author_count_with_title, NormalizedTitle};
    if !matches!(matched.provider_id.as_str(), "library_of_congress" | "wikidata" | OPEN_LIBRARY_WORK_PROVIDER) || matched.record_ids.is_empty() || matched.record_ids.len() > 128 || matched.record_ids.iter().any(|id| id.is_empty() || id.len() > 256) {
        return None;
    }
    if matched.provider_id == OPEN_LIBRARY_WORK_PROVIDER && matched.record_ids.iter().any(|id| !id.strip_prefix("OL").and_then(|id| id.strip_suffix('W')).is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && id.parse::<u64>().is_ok_and(|id| id > 0))) {
        return None;
    }
    if matched.provider_id == "wikidata" && matched.record_ids.iter().any(|id| !id.strip_prefix('Q').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && n.parse::<u64>().is_ok_and(|n| n > 0))) {
        return None;
    }
    let local = NormalizedTitle::new(&query.title);
    let remote = NormalizedTitle::new(&matched.title);
    let main = NormalizedTitle::new(&matched.main_title);
    if local.full.is_empty() || !(local.full == remote.full || local.full == main.full || local.main == remote.full) {
        return None;
    }
    if local.qualifiers != remote.qualifiers {
        return None;
    }
    if matching_author_count_with_title(&query.authors, &matched.authors, &query.title, &matched.title) == 0 {
        return None;
    }
    let mut classifications = matched.classifications.clone();
    classifications.retain(|c| c.scheme == ClassificationScheme::LibraryOfCongress && crate::classification_consensus_notation(c.scheme, &c.notation).is_some());
    if classifications.is_empty() {
        return None;
    }
    for c in &mut classifications {
        c.source = ClassificationSource::Work;
        let supplied = &c.evidence;
        let ids = matched.record_ids.iter().filter(|id| supplied.is_empty() || supplied.iter().any(|e| e.method == format!("{}_title_author:{id}", matched.provider_id))).collect::<Vec<_>>();
        c.evidence = ids
            .into_iter()
            .map(|id| ClassificationEvidence {
                // The method prefix identifies the lookup, and its suffix preserves
                // the MARC control number even when no ISBN exists.
                method: format!("{}_title_author:{id}", matched.provider_id),
                isbn13: String::new(),
                open_library_work_id: if matched.provider_id == OPEN_LIBRARY_WORK_PROVIDER { (*id).clone() } else { String::new() },
                open_library_edition_id: String::new(),
            })
            .collect();
    }
    let work_id = (matched.provider_id == OPEN_LIBRARY_WORK_PROVIDER && matched.record_ids.len() == 1).then(|| matched.record_ids[0].clone());
    Some(RichWorkMetadataMatch { open_library_work_id: work_id, exact_edition_ids: Vec::new(), classifications, authors: Vec::new(), description: None, subjects: Vec::new(), popularity: None })
}
