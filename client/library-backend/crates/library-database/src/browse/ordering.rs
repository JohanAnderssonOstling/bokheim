//! Stable alphabetical and book-count ordering for browse sections.
use crate::browse::BrowseRow;
use library_model::BrowseChipSort;

pub(super) fn sort_rows(rows: &mut [BrowseRow], sort: BrowseChipSort) {
    if sort == BrowseChipSort::DefaultOrder {
        return;
    }
    rows.sort_by_cached_key(|row| (book_model::normalize_search_text(&row.name), row.name.clone(), row.id));
    if sort == BrowseChipSort::BookCount {
        rows.sort_by_key(|row| std::cmp::Reverse(row.book_count));
    }
}

/// Put preferred rows first while preserving the order within both groups.
pub(super) fn prefer_rows(rows: &mut Vec<BrowseRow>, preferred: &dyn Fn(&BrowseRow) -> bool) {
    let mut first = Vec::new();
    let mut rest = Vec::new();
    for row in rows.drain(..) {
        if preferred(&row) {
            first.push(row);
        } else {
            rest.push(row);
        }
    }
    rows.extend(first);
    rows.extend(rest);
}
