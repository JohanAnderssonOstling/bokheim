//! Classification recovery for bibliographically equivalent editions with year differences.
use crate::{
    matching::{author_match_keys, author_names_compatible, bibliographic_match_key},
    Classification, ClassificationEvidence, ClassificationScheme, ClassificationSource, EditionIdentityQuery, EditionIdentityResult, EditionIdentityStatus,
};
use std::collections::BTreeSet;

pub const METHOD: &str = "verified_year_only_edition_consensus";

fn credits_match(local: &[String], remote: &[String]) -> bool {
    if local.is_empty() || local.len() != remote.len() {
        return false;
    }
    let mut remaining = remote.to_vec();
    for name in local {
        let keys = author_match_keys(name);
        if keys.iter().all(|k| k.split_whitespace().count() < 2) {
            return false;
        }
        let Some(i) = remaining.iter().position(|n| author_names_compatible(name, n)) else {
            return false;
        };
        remaining.remove(i);
    }
    remaining.is_empty()
}
fn publishers(values: &[String]) -> BTreeSet<String> {
    values.iter().map(|p| bibliographic_match_key(p)).filter(|p| !p.is_empty()).collect()
}

pub fn classifications(query: &EditionIdentityQuery, result: &EditionIdentityResult) -> Vec<Classification> {
    if result.status != EditionIdentityStatus::Ambiguous || result.query_id != query.query_id || result.candidates_truncated || result.candidates.len() < 2 {
        return Vec::new();
    }
    let title = bibliographic_match_key(&query.title);
    let first = &result.candidates[0];
    let publisher = publishers(&first.publishers);
    if title.len() < 5 || publisher.is_empty() {
        return Vec::new();
    }
    let mut common = None;
    let mut evidence = Vec::new();
    for (index, candidate) in result.candidates.iter().enumerate() {
        if candidate.work_title.as_deref().map(bibliographic_match_key) != first.work_title.as_deref().map(bibliographic_match_key)
            || bibliographic_match_key(&candidate.title) != title
            || !credits_match(&query.authors, &candidate.authors)
            || result.candidates[..index].iter().any(|previous| !credits_match(&previous.authors, &candidate.authors))
            || publishers(&candidate.publishers) != publisher
        {
            return Vec::new();
        }
        // Require edition-level evidence; repeated work-wide unions cannot vote.
        let codes = candidate
            .classifications
            .iter()
            .filter(|c| c.scheme == ClassificationScheme::LibraryOfCongress)
            .filter_map(|c| crate::classification_consensus_notation(c.scheme, &c.notation))
            .collect::<BTreeSet<_>>();
        if codes.is_empty() || common.as_ref().is_some_and(|previous| previous != &codes) {
            return Vec::new();
        }
        common = Some(codes);
        evidence.push(ClassificationEvidence {
            method: METHOD.into(),
            isbn13: candidate.canonical_isbn13.clone(),
            open_library_work_id: candidate.open_library_work_id.clone().unwrap_or_default(),
            open_library_edition_id: candidate.open_library_edition_id.clone(),
        });
    }
    common.unwrap_or_default().into_iter().map(|notation| Classification { notation, scheme: ClassificationScheme::LibraryOfCongress, source: ClassificationSource::Work, evidence: evidence.clone() }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EditionIdentityMatch, EditionTitleMatch};
    fn fixture() -> (EditionIdentityQuery, EditionIdentityResult) {
        let query = EditionIdentityQuery { query_id: "1".into(), title: "Example History: Volume 2".into(), authors: vec!["Ada Author".into()], publishers: vec![], book_year: None };
        let candidates = [1999, 2013]
            .into_iter()
            .map(|year| EditionIdentityMatch { subtitle: None,
                canonical_isbn13: if year == 1999 { "9780821338278" } else { "9780393243277" }.into(),
                open_library_edition_id: format!("OL{year}M"),
                open_library_work_id: Some(format!("OL{year}W")),
                title: query.title.clone(),
                work_title: Some(query.title.clone()),
                title_match: EditionTitleMatch::Both,
                authors: query.authors.clone(),
                publishers: vec!["Example Press".into()],
                book_year: Some(year),
                matched_publisher: false,
                matched_book_year: false,
                classifications: vec![Classification { scheme: ClassificationScheme::LibraryOfCongress, notation: format!("E806 .S89 {year}"), source: ClassificationSource::ExactEdition, evidence: vec![] }],
            })
            .collect();
        (query, EditionIdentityResult { catalog_subjects: None, query_id: "1".into(), status: EditionIdentityStatus::Ambiguous, matched: None, candidates, candidates_truncated: false })
    }
    #[test]
    fn work_classifications_participate_in_agreement() {
        let (q, mut r) = fixture();
        let expected = classifications(&q, &r);
        assert!(!expected.is_empty());
        r.candidates[1].classifications[0].source = ClassificationSource::Work;
        assert_eq!(classifications(&q, &r), expected);
        r.candidates[0].classifications[0].source = ClassificationSource::Work;
        assert_eq!(classifications(&q, &r), expected);
    }

    #[test]
    fn year_difference_yields_work_classification_with_both_evidence_records() {
        let (q, r) = fixture();
        let codes = classifications(&q, &r);
        assert_eq!(codes.len(), 1);
        assert_eq!(codes[0].notation, "E806");
        assert_eq!(codes[0].source, ClassificationSource::Work);
        assert_eq!(codes[0].evidence.len(), 2);
        assert_ne!(codes[0].evidence[0].isbn13, codes[0].evidence[1].isbn13);
        assert!(r.matched.is_none());
    }
    #[test]
    fn rejects_identity_and_classification_differences() {
        for change in 0..9 {
            let (mut q, mut r) = fixture();
            match change {
                0 => r.candidates[1].title = "Example History: Volume 3".into(),
                1 => r.candidates[1].title.push_str(" Revised edition"),
                2 => r.candidates[1].authors = vec!["Adam Author".into()],
                3 => r.candidates[1].publishers = vec!["Other Press".into()],
                4 => r.candidates[1].classifications[0].notation = "E807 .S90 2013".into(),
                5 => r.candidates[1].classifications[0].notation = "invalid".into(),
                6 => r.candidates_truncated = true,
                7 => q.authors.clear(),
                _ => r.candidates[1].classifications.clear(),
            }
            assert!(classifications(&q, &r).is_empty(), "change {change}");
        }
    }

    #[test]
    fn omitted_initial_resolves_but_conflicting_candidate_initials_do_not() {
        let (mut q, mut r) = fixture();
        q.authors = vec!["Brian Klaas".into()];
        r.candidates[0].authors = vec!["Brian Klaas".into()];
        r.candidates[1].authors = vec!["Brian P. Klaas".into()];
        assert!(!classifications(&q, &r).is_empty());
        let mut conflicting = r.candidates[1].clone();
        conflicting.authors = vec!["Brian Q. Klaas".into()];
        r.candidates.push(conflicting);
        assert!(classifications(&q, &r).is_empty());
    }
}
