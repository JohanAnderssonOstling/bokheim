//! Read immutable leaf evidence and derive totals over unique, filtered members.
use super::books::MatchingBooks;
use crate::browse::BrowseRow;
use library_model::BrowseChipSort;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
#[cfg(test)]
mod tests;

pub(super) fn sum(members: &BTreeSet<i64>, matching: &MatchingBooks) -> Option<f64> {
    let source = source_priority(members, matching);
    let mut groups = BTreeMap::<&str, f64>::new();
    let mut total = None;
    for id in members.intersection(&matching.books) {
        // Raw counts from different sources have different scales. A catalog
        // group uses only the books in its highest-priority source tier.
        if source.is_some() && matching.source_priorities.get(id).copied() != source {
            continue;
        }
        let Some(&score) = matching.popularity.get(id) else { continue };
        if let Some(group) = matching.popularity_groups.get(id) {
            groups.entry(group).and_modify(|previous| *previous = previous.max(score)).or_insert(score);
        } else {
            total = Some(total.unwrap_or(0.0) + score);
        }
    }
    groups.values().fold(total, |total, score| Some(total.unwrap_or(0.0) + score))
}

pub(super) fn source_priority(members: &BTreeSet<i64>, matching: &MatchingBooks) -> Option<u8> {
    members.intersection(&matching.books).filter_map(|id| matching.source_priorities.get(id)).copied().min()
}

/// Descending known scores, followed by unknowns. A known zero stays known.
pub(super) fn compare(left: Option<f64>, right: Option<f64>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => right.total_cmp(&left),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

pub(super) fn sort_rows(rows: &mut [BrowseRow], sort: BrowseChipSort, score: impl Fn(&BrowseRow) -> Option<f64>) {
    if sort == BrowseChipSort::DefaultOrder {
        return;
    }
    let scores: BTreeMap<_, _> = if sort == BrowseChipSort::Popularity { rows.iter().map(|row| (row.id, score(row))).collect() } else { BTreeMap::new() };
    rows.sort_by_cached_key(|row| (book_model::normalize_search_text(&row.name), row.name.clone(), row.id));
    match sort {
        BrowseChipSort::Popularity => rows.sort_by(|left, right| compare(scores.get(&left.id).copied().flatten(), scores.get(&right.id).copied().flatten())),
        BrowseChipSort::BookCount => rows.sort_by_key(|row| std::cmp::Reverse(row.book_count)),
        BrowseChipSort::Alphabetical | BrowseChipSort::DefaultOrder => {}
    }
}
