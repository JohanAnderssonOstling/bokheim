use super::*;

#[test]
fn folder_groups_expand_once_preserve_empty_chips_and_ignore_filtered_direct_books() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/chip_group_tests/folder_groups_expand_once_preserve_empty_chips_and_ignore_filtered_direct_books_insert.sql")).unwrap();
    let root = "00000000-0000-0000-0000-000000000000";
    let mut rows = get_folder_list_rows_filtered_sorted(&conn, root, "", &[], BrowseChipSort::Alphabetical, &[]).unwrap();
    let groups = crate::browse::group_folder_children(&conn, &mut rows, &[], &[], BrowseChipSort::Alphabetical, false).unwrap();
    assert_eq!(rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Empty", "Mixed", "Religion"]);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].parent.name, "Humanities");
    assert_eq!((groups[0].start, groups[0].end), (2, 3));
    assert_eq!(rows[2].id, "00000000-0000-0000-0000-000000000002");
    assert_eq!(rows[2].book_count, 1);
    assert_eq!(rows[2].downloaded_book_count, 1);
    let mut filtered = get_folder_list_rows_filtered_sorted(&conn, root, "", &[LibraryFileTypeFilter::Book], BrowseChipSort::Alphabetical, &[]).unwrap();
    let groups = crate::browse::group_folder_children(&conn, &mut filtered, &[LibraryFileTypeFilter::Book], &[], BrowseChipSort::Alphabetical, false).unwrap();
    assert_eq!(groups.len(), 1);
    assert!(filtered.iter().any(|r| r.name == "Mixed"), "PDF direct book still prevents expansion under an EPUB filter");
    assert!(!filtered.iter().any(|r| r.name == "Child"));
    let mut inside = get_folder_list_rows_filtered_sorted(&conn, &groups[0].parent.id, "", &[], BrowseChipSort::Alphabetical, &[]).unwrap();
    let groups = crate::browse::group_folder_children(&conn, &mut inside, &[], &[], BrowseChipSort::Alphabetical, true).unwrap();
    assert_eq!(groups[0].parent.name, "Religion");
    assert_eq!(inside[0].name, "Buddhism");
}

#[test]
fn complete_folder_views_apply_download_scope_before_grouping() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(include_str!("sql/chip_group_tests/folder_groups_expand_once_preserve_empty_chips_and_ignore_filtered_direct_books_insert.sql")).unwrap();
    let root = "00000000-0000-0000-0000-000000000000";
    let contents = crate::browse::folder_contents_filtered_sorted(&conn, root, "", &[], BrowseChipSort::BookCount, library_model::BrowseBookSort::Alphabetical, &[], true).unwrap();
    assert_eq!(contents.children.len(), 1);
    assert_eq!(contents.children[0].name, "Humanities");
    assert_eq!(contents.children[0].book_count, 1);
    let mut query = library_model::LibraryBrowseQuery {
        location: root.into(),
        search: String::new(),
        file_types: vec![],
        chip_sort: BrowseChipSort::Alphabetical,
        book_sort: library_model::BrowseBookSort::Alphabetical,
        languages: vec![],
        hide_finished: false,
        include_direct_child_books: false,
    };
    let (contents, groups) = crate::browse::grouped_folder_contents(&conn, &query, true).unwrap();
    assert_eq!(contents.children.len(), 1);
    assert_eq!(contents.children[0].name, "Religion");
    assert_eq!(groups.len(), 1);
    query.search = "Religion".into();
    let (_, groups) = crate::browse::grouped_folder_contents(&conn, &query, true).unwrap();
    assert!(groups.is_empty(), "search results must not expand folder groups");
}
