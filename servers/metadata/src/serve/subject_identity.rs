//! Identity checks shared by sibling retrieval and subject voting.
use metadata_contract::matching::{author_credits, author_name_match_key, author_names_with_omitted_names, matching_author_count_with_title, title_qualifiers};
use metadata_contract::subject_consensus::titles_compatible;
use metadata_contract::{EditionIdentityMatch, EditionIdentityQuery};
use std::collections::BTreeSet;

pub(super) type Candidate = (usize, usize, usize, EditionIdentityMatch);

/// Keep the most informative spelling when the source repeats one author.
/// Different explicit full names remain separate constraints.
pub(super) fn authors(values: &[String]) -> Vec<String> {
    let credits = author_credits(values);
    if credits.len() < 2 { return credits; }
    credits.iter().filter(|name| {
        let key = author_name_match_key(name);
        let information = (key.split_whitespace().count(), key.chars().count());
        !credits.iter().any(|other| {
            let other_key = author_name_match_key(other);
            (other_key.split_whitespace().count(), other_key.chars().count()) > information
                && author_names_with_omitted_names(name, other)
        })
    }).cloned().collect()
}

fn work_title_agrees(query: &EditionIdentityQuery, candidate: &EditionIdentityMatch) -> bool {
    candidate.work_title.as_deref().is_some_and(|title| titles_compatible(&query.title, title))
}

pub(super) fn title_agrees(query: &EditionIdentityQuery, candidate: &EditionIdentityMatch, works: &BTreeSet<String>) -> bool {
    title_qualifiers(&query.title) == title_qualifiers(&candidate.full_title())
        && (titles_compatible(&query.title, &candidate.full_title()) || work_title_agrees(query, candidate)
            || candidate.open_library_work_id.as_ref().is_some_and(|work| works.contains(work)))
}

pub(super) fn author_count(query: &EditionIdentityQuery, candidate: &EditionIdentityMatch, works: &BTreeSet<String>) -> usize {
    let full_title = candidate.full_title();
    // A verified work supplies title aliases, including translations. It does
    // not supply an edition identity or classifications of its own.
    let title = if work_title_agrees(query, candidate) || candidate.open_library_work_id.as_ref().is_some_and(|work| works.contains(work)) {
        &query.title
    } else {
        &full_title
    };
    matching_author_count_with_title(&authors(&query.authors), &authors(&candidate.authors), &query.title, title)
}

pub(super) fn verified_works(query: &EditionIdentityQuery, candidates: &[Candidate]) -> BTreeSet<String> {
    let none = BTreeSet::new();
    candidates.iter().filter(|(_, _, _, candidate)| title_agrees(query, candidate, &none) && author_count(query, candidate, &none) > 0)
        .filter_map(|(_, _, _, candidate)| candidate.open_library_work_id.clone()).collect()
}

pub(super) fn conflicting_authors(query: &EditionIdentityQuery, candidates: &[&EditionIdentityMatch]) -> bool {
    for local in authors(&query.authors) {
        let credited = candidates.iter().flat_map(|candidate| authors(&candidate.authors))
            .filter(|remote| author_names_with_omitted_names(&local, remote)).collect::<BTreeSet<_>>();
        // Different omissions can look incompatible with each other while
        // agreeing with a longer stated name: Gaius Caesar and Julius Caesar
        // both fit Gaius Julius Caesar. Compare the maximal constraints.
        let credited = authors(&credited.into_iter().chain(std::iter::once(local)).collect::<Vec<_>>());
        if credited.iter().enumerate().any(|(index, name)| credited.iter().take(index)
            .any(|other| !author_names_with_omitted_names(name, other))) {
            return true;
        }
    }
    false
}
