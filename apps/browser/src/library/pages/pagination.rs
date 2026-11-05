//! Page compatibility, missing-range selection and indexed merges shared by library pages.
use library_model::{BrowsePageRequest, BrowsePageState};
use std::{collections::BTreeMap, ops::Range};

pub(super) fn compatible_pages(current: &BrowsePageState, next: &BrowsePageState) -> bool {
    current.result_revision == next.result_revision
        && current.book_total == next.book_total
        && match (&current.headings, &next.headings) {
            (Some(current), Some(next)) => current == next,
            (None, None) => current.section_ids == next.section_ids,
            _ => false,
        }
}

pub(super) fn page_request_for_slots(slots: Range<usize>, compact: usize, sections: usize, books: usize, section_loaded: impl Fn(usize) -> bool, book_loaded: impl Fn(usize) -> bool) -> Option<BrowsePageRequest> {
    let mut section_range = slots.start.saturating_sub(compact).min(sections)..slots.end.saturating_sub(compact).min(sections);
    let section = section_range.find(|index| !section_loaded(*index));
    let books_start = compact + sections;
    let mut book_range = slots.start.saturating_sub(books_start).min(books)..slots.end.saturating_sub(books_start).min(books);
    let book = book_range.find(|index| !book_loaded(*index));
    if section.is_none() && book.is_none() {
        return None;
    }
    Some(BrowsePageRequest { section_offset: section.unwrap_or(0), section_limit: if section.is_some() { 24 } else { 0 }, book_offset: book.unwrap_or(0), book_limit: if book.is_some() { 48 } else { 0 } })
}

/// Materialize each position once, retaining entities already installed there.
pub(super) fn merge_page<T, R>(items: &mut BTreeMap<usize, T>, offset: usize, rows: impl IntoIterator<Item = R>, mut create: impl FnMut(R) -> T) {
    for (index, row) in (offset..).zip(rows) {
        items.entry(index).or_insert_with(|| create(row));
    }
}
