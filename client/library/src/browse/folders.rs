//! Complete folder views share filtering, sorting, and grouping on every host.
use super::*;
use library_model::BrowseContents;

pub fn folder_contents_filtered_sorted(
    conn: &Connection, folder_id: &str, query: &str, file_types: &[LibraryFileTypeFilter], chip_sort: BrowseChipSort, book_sort: BrowseBookSort, languages: &[&str], downloaded_only: bool,
) -> rusqlite::Result<BrowseContents> {
    let normalized_query = catalog_model::normalize_search_text(query);
    folder_contents(conn, folder_id, &normalized_query, file_types, chip_sort, book_sort, languages, downloaded_only)
}

fn folder_contents(
    conn: &Connection, folder_id: &str, normalized_query: &str, file_types: &[LibraryFileTypeFilter], chip_sort: BrowseChipSort, book_sort: BrowseBookSort, languages: &[&str], downloaded_only: bool,
) -> rusqlite::Result<BrowseContents> {
    let mut children = get_folder_list_rows_filtered_sorted(conn, folder_id, normalized_query, file_types, chip_sort, languages)?;
    let books = crate::books::get_book_cards_in_dir_filtered_sorted_scoped(conn, folder_id, downloaded_only, normalized_query, file_types, book_sort, languages)?;
    scope_browse_children(&mut children, downloaded_only, chip_sort);
    Ok(BrowseContents { children, books })
}

pub fn grouped_folder_contents(conn: &Connection, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> rusqlite::Result<(BrowseContents, Vec<library_model::BrowseChipGroup>)> {
    let tx = conn.unchecked_transaction()?;
    let folder_id = DirId::parse_str(&query.location).map_err(|error| super::invalid_text_column(0, error.to_string()))?.to_string();
    let search = catalog_model::normalize_search_text(&query.search);
    let languages = query.languages.iter().map(String::as_str).collect::<Vec<_>>();
    let mut contents = folder_contents(&tx, &folder_id, &search, &query.file_types, query.chip_sort, query.book_sort, &languages, downloaded_only)?;
    let groups = if search.is_empty() { group_folder_children(&tx, &mut contents.children, &query.file_types, &languages, query.chip_sort, downloaded_only)? } else { Vec::new() };
    tx.commit()?;
    Ok((contents, groups))
}

fn scope_browse_children(children: &mut Vec<library_model::BrowseRow>, downloaded_only: bool, sort: BrowseChipSort) {
    if downloaded_only {
        children.retain(|child| child.downloaded_book_count > 0);
        for child in children.iter_mut() {
            child.book_count = child.downloaded_book_count;
        }
        if sort == BrowseChipSort::BookCount {
            children.sort_by(|a, b| b.book_count.cmp(&a.book_count).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        }
    }
}
