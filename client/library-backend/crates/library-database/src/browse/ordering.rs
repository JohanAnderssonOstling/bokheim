//! Stable alphabetical and book-count ordering for browse sections.
use crate::browse::BrowseRow;
use library_model::BrowseChipSort;
use std::cmp::Reverse;

/// SQL over normalized book sort keys, with a stable identity tie-breaker.
/// Unknown sizes remain last in either direction.
pub(super) fn book_order(sort: library_model::BrowseBookSort) -> String {
    use library_model::BrowseBookSort;
    if matches!(sort, BrowseBookSort::Format | BrowseBookSort::FormatDescending) {
        let formats = book_model::BookFormat::ALL.into_iter().map(|format| format!("WHEN {} THEN '{}'", format as u8, format.canonical_extension())).collect::<Vec<_>>().join(" ");
        let direction = if sort == BrowseBookSort::FormatDescending { "DESC" } else { "ASC" };
        return format!("CASE format {formats} ELSE '' END {direction},search_title,title,key");
    }
    match sort {
        BrowseBookSort::RecentlyRead => "read_at DESC,search_title,title,key",
        BrowseBookSort::TitleDescending => "search_title DESC,title DESC,key",
        BrowseBookSort::Author => "search_author,search_title,title,key",
        BrowseBookSort::AuthorDescending => "search_author DESC,search_title,title,key",
        BrowseBookSort::Format | BrowseBookSort::FormatDescending => unreachable!(),
        BrowseBookSort::Size => "size_bytes IS NULL,size_bytes,search_title,title,key",
        BrowseBookSort::SizeDescending => "size_bytes IS NULL,size_bytes DESC,search_title,title,key",
        BrowseBookSort::Progress => "progress,search_title,title,key",
        BrowseBookSort::ProgressDescending => "progress DESC,search_title,title,key",
        BrowseBookSort::Availability => "downloaded,search_title,title,key",
        BrowseBookSort::AvailabilityDescending => "downloaded DESC,search_title,title,key",
        BrowseBookSort::Alphabetical | BrowseBookSort::DefaultOrder => "search_title,title,key",
    }
    .into()
}

pub(super) fn sort_rows(rows: &mut [BrowseRow], sort: BrowseChipSort) {
    if sort == BrowseChipSort::DefaultOrder {
        return;
    }
    rows.sort_by_cached_key(|row| (book_model::normalize_search_text(&row.name), row.name.clone(), row.id));
    match sort {
        BrowseChipSort::TitleDescending => rows.reverse(),
        BrowseChipSort::BookCount => rows.sort_by_key(|row| Reverse(row.book_count)),
        BrowseChipSort::Size => rows.sort_by_key(|row| (row.size_bytes.is_none(), row.size_bytes)),
        BrowseChipSort::SizeDescending => rows.sort_by_key(|row| (row.size_bytes.is_none(), Reverse(row.size_bytes))),
        BrowseChipSort::Availability | BrowseChipSort::AvailabilityDescending => rows.sort_by(|a, b| {
            let a_downloaded = a.downloaded_book_count as u128 * b.book_count.max(1) as u128;
            let b_downloaded = b.downloaded_book_count as u128 * a.book_count.max(1) as u128;
            if sort == BrowseChipSort::Availability { a_downloaded.cmp(&b_downloaded) } else { b_downloaded.cmp(&a_downloaded) }
        }),
        BrowseChipSort::Alphabetical | BrowseChipSort::DefaultOrder => {}
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
