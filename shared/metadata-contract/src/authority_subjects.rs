//! Classification-only authority evidence, independent of an ISBN or OL identity.
use crate::{AuthoritySubjectMatch, ClassificationEvidence, ClassificationScheme, ClassificationSource, EditionIdentityQuery, RichWorkMetadataMatch};
pub const METHOD_PREFIX: &str = "library_of_congress_title_author:";

pub fn classification_record(query: &EditionIdentityQuery, matched: &AuthoritySubjectMatch) -> Option<RichWorkMetadataMatch> {
    use crate::matching::{author_credits, author_names_compatible, NormalizedTitle};
    if !matches!(matched.provider_id.as_str(), "library_of_congress" | "wikidata") || matched.record_ids.is_empty() || matched.record_ids.len() > 128 || matched.record_ids.iter().any(|id| id.is_empty() || id.len() > 256) {
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
    let authors = author_credits(&query.authors);
    if !authors.iter().any(|local| matched.authors.iter().any(|remote| author_names_compatible(local, remote))) {
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
                open_library_work_id: String::new(),
                open_library_edition_id: String::new(),
            })
            .collect();
    }
    Some(RichWorkMetadataMatch { open_library_work_id: None, exact_edition_ids: Vec::new(), classifications, authors: Vec::new(), description: None, subjects: Vec::new() })
}
