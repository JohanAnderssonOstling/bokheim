//! Classification recovery for bibliographically equivalent editions with year differences.
use crate::{
    matching::{bibliographic_match_key, matching_author_count, NormalizedTitle},
    Classification, EditionIdentityQuery, EditionIdentityResult, EditionIdentityStatus,
};
use std::collections::BTreeSet;

pub const METHOD: &str = "verified_year_variant_classifications";

fn credits_match(local: &[String], remote: &[String]) -> bool {
    matching_author_count(local, remote) > 0
}
fn publishers(values: &[String]) -> BTreeSet<String> {
    values.iter().map(|p| bibliographic_match_key(p)).filter(|p| !p.is_empty()).collect()
}

pub fn classifications(query: &EditionIdentityQuery, result: &EditionIdentityResult) -> Vec<Classification> {
    if result.status != EditionIdentityStatus::Ambiguous || result.query_id != query.query_id || result.candidates_truncated || result.candidates.len() < 2 {
        return Vec::new();
    }
    let title = NormalizedTitle::new(&query.title);
    let first = &result.candidates[0];
    let publisher = publishers(&first.publishers);
    if title.full.len() < 5 || publisher.is_empty() {
        return Vec::new();
    }
    for (index, candidate) in result.candidates.iter().enumerate() {
        if candidate.work_title.as_deref().map(bibliographic_match_key) != first.work_title.as_deref().map(bibliographic_match_key)
            || !title.matches(&NormalizedTitle::new(&candidate.full_title()))
            || !credits_match(&query.authors, &candidate.authors)
            || result.candidates[..index].iter().any(|previous| !credits_match(&previous.authors, &candidate.authors))
            || publishers(&candidate.publishers) != publisher
        {
            return Vec::new();
        }
    }
    crate::subject_consensus::candidate_classifications(result.candidates.iter(), METHOD)
}
