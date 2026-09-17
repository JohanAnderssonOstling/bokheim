use super::*;
use crate::browse::grouped_subject_contents;
use library_model::{BrowseContents, LibraryBrowseQuery};
use subject_projection::ROOT_SUBJECT_PATH;

fn fixture() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    use_fixture_taxonomy(&conn);
    conn.execute_batch(include_str!("sql/subject_compaction_tests/fixture_insert.sql")).unwrap();
    conn
}

fn direct_fixture() -> Connection {
    let conn = fixture();
    conn.execute("UPDATE book_unified_concept SET concept_id=910000", []).unwrap();
    conn
}

fn query(location: &str) -> LibraryBrowseQuery {
    LibraryBrowseQuery {
        location: location.into(),
        search: String::new(),
        file_types: vec![LibraryFileTypeFilter::All],
        chip_sort: BrowseChipSort::Alphabetical,
        book_sort: BrowseBookSort::Alphabetical,
        languages: Vec::new(),
        hide_finished: false,
        include_direct_child_books: false,
    }
}

fn browse(conn: &Connection, location: &str) -> BrowseContents {
    compacted_subject_contents(conn, &query(location), false).unwrap()
}

fn titles(contents: &BrowseContents) -> Vec<&str> {
    contents.books.iter().map(|book| book.title.as_str()).collect()
}

fn children(contents: &BrowseContents) -> Vec<&str> {
    contents.children.iter().map(|child| child.id.as_str()).collect()
}

#[test]
fn virtual_subject_root_is_built_filtered_and_repaired() {
    let conn = fixture();
    // Production routes reference the virtual root without storing it as a
    // curated concept. "World" is also shorter than "Subject".
    conn.execute("UPDATE curated.unified_concept_route SET parent_path=?1 WHERE path='World'", [ROOT_SUBJECT_PATH]).unwrap();
    let root = browse(&conn, ROOT_SUBJECT_PATH);
    assert_eq!(children(&root), ["World"]);
    assert_eq!(root.children[0].book_count, 3);
    let mut filtered = query(ROOT_SUBJECT_PATH);
    filtered.languages = vec!["en".into()];
    let root = compacted_subject_contents(&conn, &filtered, false).unwrap();
    assert_eq!(children(&root), ["World"]);
    assert_eq!(root.children[0].book_count, 2);
    let root = compacted_subject_contents(&conn, &query(ROOT_SUBJECT_PATH), true).unwrap();
    assert_eq!(children(&root), ["World"]);
    assert_eq!(root.children[0].book_count, 1);

    // Simulate the already-built cache produced by the old implementation.
    conn.execute("DELETE FROM library_subject WHERE path=?1 OR path=''", [ROOT_SUBJECT_PATH]).unwrap();
    conn.execute("DELETE FROM subject_browse_member WHERE path=?1 OR path=''", [ROOT_SUBJECT_PATH]).unwrap();
    conn.execute("DELETE FROM subject_browse_child WHERE location=?1 OR location=''", [ROOT_SUBJECT_PATH]).unwrap();
    let repaired = browse(&conn, ROOT_SUBJECT_PATH);
    assert_eq!(children(&repaired), ["World"]);
    assert_eq!(repaired.children[0].book_count, 3);
}

#[test]
fn subject_hierarchy_retains_every_level_and_direct_book_placement() {
    let conn = fixture();
    let before: i64 = conn.query_row("SELECT count(*) FROM book_unified_concept", [], |r| r.get(0)).unwrap();
    let contents = browse(&conn, "World / U.S.");
    assert_eq!(children(&contents), ["World / U.S. / A", "World / U.S. / B", "World / U.S. / C"]);
    assert!(contents.books.is_empty());
    assert_eq!(children(&browse(&conn, "World")), ["World / U.S."]);
    assert_eq!(children(&browse(&conn, "World / U.S. / A")), ["World / U.S. / A / Leaf"]);
    assert_eq!(titles(&browse(&conn, "World / U.S. / A / Leaf")), ["Alpha book"]);
    assert_eq!(conn.query_row("SELECT count(*) FROM book_unified_concept", [], |r| r.get::<_, i64>(0)).unwrap(), before);
}

#[test]
fn subject_hierarchy_does_not_change_depth_with_book_count() {
    let conn = fixture();
    conn.execute(include_str!("sql/subject_compaction_tests/compaction_deduplicates_books_across_shared_subject_paths_delete.sql"), []).unwrap();
    assert_eq!(children(&browse(&conn, "World / U.S.")), ["World / U.S. / A"]);
    assert!(browse(&conn, "World / U.S.").books.is_empty());
    conn.execute(include_str!("sql/subject_compaction_tests/compaction_reverses_when_a_second_book_or_subject_appears_insert.sql"), []).unwrap();
    assert_eq!(children(&browse(&conn, "World / U.S.")), ["World / U.S. / A"]);
    assert_eq!(titles(&browse(&conn, "World / U.S. / A / Leaf")), ["Alpha book", "Beta book"]);
    conn.execute(include_str!("sql/subject_compaction_tests/compaction_reverses_when_a_second_book_or_subject_appears_update.sql"), []).unwrap();
    assert_eq!(children(&browse(&conn, "World / U.S.")), ["World / U.S. / A", "World / U.S. / B"]);
    assert!(browse(&conn, "World / U.S.").books.is_empty());
}

#[test]
fn compaction_counts_general_books_and_subjects_together_and_preserves_specificity() {
    let conn = fixture();
    conn.execute(include_str!("sql/subject_compaction_tests/compaction_counts_general_books_and_subjects_together_and_preserves_specificity_delete.sql"), []).unwrap();
    conn.execute(include_str!("sql/subject_compaction_tests/compaction_counts_general_books_and_subjects_together_and_preserves_specificity_update.sql"), []).unwrap();
    let mixed = browse(&conn, "World / U.S.");
    assert_eq!(titles(&mixed), ["Beta book"]);
    assert_eq!(children(&mixed), ["World / U.S. / A"], "the general book must not absorb the specific child book");
    assert_eq!(children(&browse(&conn, "World / U.S.")), ["World / U.S. / A"]);
    let specific = browse(&conn, "World / U.S. / A");
    assert_eq!(children(&specific), ["World / U.S. / A / Leaf"]);
    assert!(specific.books.is_empty());
    assert_eq!(titles(&browse(&conn, "World / U.S. / A / Leaf")), ["Alpha book"]);
    conn.execute(include_str!("sql/subject_compaction_tests/compaction_counts_general_books_and_subjects_together_and_preserves_specificity_delete_2.sql"), []).unwrap();
    assert!(browse(&conn, "World / U.S.").books.is_empty());
}

#[test]
fn compaction_uses_format_language_download_and_finished_filters_before_counting() {
    let conn = direct_fixture();
    conn.execute(include_str!("sql/subject_compaction_tests/headings_expand_one_level_after_singleton_branches_are_compacted_update.sql"), []).unwrap();
    let mut filtered = query("World / U.S.");
    assert_eq!(compacted_subject_contents(&conn, &filtered, false).unwrap().books.len(), 3);
    filtered.file_types = vec![LibraryFileTypeFilter::Book];
    assert_eq!(titles(&compacted_subject_contents(&conn, &filtered, false).unwrap()), ["Alpha book"]);
    filtered.file_types.push(LibraryFileTypeFilter::Pdf);
    assert_eq!(compacted_subject_contents(&conn, &filtered, false).unwrap().books.len(), 2);
    filtered.file_types = vec![LibraryFileTypeFilter::All];
    filtered.languages = vec!["sv".into()];
    assert_eq!(titles(&compacted_subject_contents(&conn, &filtered, false).unwrap()), ["Beta book"]);
    filtered.languages = vec!["en".into()];
    assert_eq!(compacted_subject_contents(&conn, &filtered, false).unwrap().books.len(), 2);
    filtered.hide_finished = true;
    assert_eq!(titles(&compacted_subject_contents(&conn, &filtered, false).unwrap()), ["Alpha book"]);
    filtered.languages.clear();
    filtered.hide_finished = false;
    assert_eq!(titles(&compacted_subject_contents(&conn, &filtered, true).unwrap()), ["Alpha book"]);
    assert_eq!(compacted_subject_contents(&conn, &filtered, false).unwrap().books.len(), 3, "filters never alter stored classification");
}

#[test]
fn subject_counts_deduplicate_books_across_shared_paths() {
    let conn = fixture();
    conn.execute(include_str!("sql/subject_compaction_tests/compaction_deduplicates_books_across_shared_subject_paths_delete.sql"), []).unwrap();
    conn.execute(include_str!("sql/subject_compaction_tests/compaction_deduplicates_books_across_shared_subject_paths_update.sql"), []).unwrap();
    let contents = browse(&conn, "World / U.S.");
    assert_eq!(children(&contents), ["World / U.S. / A", "World / U.S. / B"]);
    assert!(contents.books.is_empty());
    assert_eq!(browse(&conn, "World").children[0].book_count, 1);
    assert_eq!(titles(&browse(&conn, "World / U.S. / B / Leaf")), ["Alpha book"]);
}

#[test]
fn direct_child_mode_includes_children_but_not_grandchildren() {
    let conn = fixture();
    conn.execute(include_str!("sql/subject_compaction_tests/direct_child_mode_includes_children_but_not_grandchildren_update.sql"), []).unwrap();
    conn.execute(include_str!("sql/subject_compaction_tests/direct_child_mode_includes_children_but_not_grandchildren_update_2.sql"), []).unwrap();

    let direct = compacted_subject_contents(&conn, &query("World / U.S."), false).unwrap();
    assert_eq!(titles(&direct), ["Gamma book"]);

    let mut with_children = query("World / U.S.");
    with_children.include_direct_child_books = true;
    let with_children = compacted_subject_contents(&conn, &with_children, false).unwrap();
    assert_eq!(titles(&with_children), ["Alpha book", "Gamma book"]);
    assert!(!titles(&with_children).contains(&"Beta book"), "a grandchild's book must remain excluded");
}

#[test]
fn compaction_preserves_scoped_subject_and_book_search() {
    let conn = fixture();
    let mut search = query("World / U.S.");
    search.search = "branch".into();
    let contents = compacted_subject_contents(&conn, &search, false).unwrap();
    assert_eq!(contents.children.len(), 3);
    assert!(contents.books.is_empty());
    search.search = "alpha".into();
    assert_eq!(titles(&compacted_subject_contents(&conn, &search, false).unwrap()), ["Alpha book"]);
    search.location = "World / U.S. / B".into();
    assert!(compacted_subject_contents(&conn, &search, false).unwrap().books.is_empty());
    search.location = "World / U.S.".into();
    search.search = "branch a".into();
    conn.execute(include_str!("sql/subject_compaction_tests/compaction_preserves_scoped_subject_and_book_search_update.sql"), []).unwrap();
    let contents = compacted_subject_contents(&conn, &search, false).unwrap();
    assert_eq!(children(&contents), ["World / U.S. / A"]);
    assert_eq!(contents.children[0].path.as_deref(), Some("World / U.S."));
    assert_eq!(contents.children[0].book_count, 2);
}

#[test]
fn compaction_preserves_global_book_order_after_promoting_siblings() {
    let conn = direct_fixture();
    conn.execute_batch(include_str!("sql/subject_compaction_tests/compaction_preserves_global_book_order_after_promoting_siblings_insert.sql")).unwrap();
    let mut sorted = query("World / U.S.");
    sorted.book_sort = BrowseBookSort::RecentlyRead;
    assert_eq!(titles(&compacted_subject_contents(&conn, &sorted, false).unwrap()), ["Gamma book", "Beta book", "Alpha book"]);
    sorted.book_sort = BrowseBookSort::Alphabetical;
    assert_eq!(titles(&compacted_subject_contents(&conn, &sorted, false).unwrap()), ["Alpha book", "Beta book", "Gamma book"]);
}

#[test]
fn single_populated_branch_is_not_skipped() {
    let conn = fixture();
    conn.execute(include_str!("sql/subject_compaction_tests/compaction_skips_the_open_pages_only_subject_until_a_second_choice_appears_update.sql"), []).unwrap();
    assert_eq!(children(&browse(&conn, "World / U.S.")), ["World / U.S. / A"]);
    assert!(browse(&conn, "World / U.S.").books.is_empty());
    assert_eq!(children(&browse(&conn, "World / U.S. / A")), ["World / U.S. / A / Leaf"]);
    assert_eq!(titles(&browse(&conn, "World / U.S. / A / Leaf")), ["Alpha book", "Beta book", "Gamma book"]);
}

#[test]
fn stored_compaction_and_filtered_views_do_not_read_the_taxonomy() {
    let conn = fixture();
    let expected = browse(&conn, "World / U.S.");
    let route_count: i64 = conn.query_row(include_str!("sql/subject_compaction_tests/stored_compaction_and_filtered_views_do_not_read_the_taxonomy_select.sql"), [], |r| r.get(0)).unwrap();
    assert_eq!(route_count, 8, "Only assigned subjects and their ancestors are materialized");
    conn.execute_batch("DETACH DATABASE curated; PRAGMA query_only=ON;").unwrap();
    let before = conn.total_changes();
    assert_eq!(browse(&conn, "World / U.S."), expected);
    let mut filtered = query("World / U.S.");
    filtered.languages = vec!["en".into()];
    assert_eq!(children(&compacted_subject_contents(&conn, &filtered, false).unwrap()), ["World / U.S. / A", "World / U.S. / C"]);
    assert_eq!(children(&compacted_subject_contents(&conn, &filtered, false).unwrap()), ["World / U.S. / A", "World / U.S. / C"]);
    subject_facet_counts_scoped(&conn, "World / U.S.", "leaf", &[], &[], false).unwrap();
    assert_eq!(conn.total_changes(), before, "Clean projection reads must not write");
}

#[test]
fn filtered_projection_invalidates_without_rebuilding_unfiltered_tree_for_progress() {
    let conn = direct_fixture();
    let mut filtered = query("World / U.S.");
    filtered.hide_finished = true;
    assert_eq!(compacted_subject_contents(&conn, &filtered, false).unwrap().books.len(), 3);
    let built: i64 = conn.query_row(include_str!("sql/subject_compaction_tests/upgrading_rebuilds_old_promoted_book_placements_select.sql"), [], |r| r.get(0)).unwrap();
    conn.execute(include_str!("sql/subject_compaction_tests/filtered_projection_invalidates_without_rebuilding_unfiltered_tree_for_progress_update.sql"), []).unwrap();
    assert_eq!(titles(&compacted_subject_contents(&conn, &filtered, false).unwrap()), ["Alpha book", "Gamma book"]);
    assert_eq!(conn.query_row(include_str!("sql/subject_compaction_tests/upgrading_rebuilds_old_promoted_book_placements_select.sql"), [], |r| r.get::<_, i64>(0)).unwrap(), built);
    filtered.hide_finished = false;
    filtered.languages = vec!["sv".into()];
    assert_eq!(titles(&compacted_subject_contents(&conn, &filtered, false).unwrap()), ["Beta book"]);
    conn.execute(include_str!("sql/subject_compaction_tests/filtered_projection_invalidates_without_rebuilding_unfiltered_tree_for_progress_update_2.sql"), []).unwrap();
    assert_eq!(titles(&compacted_subject_contents(&conn, &filtered, false).unwrap()), ["Beta book", "Gamma book"]);
    filtered.languages.clear();
    assert_eq!(titles(&compacted_subject_contents(&conn, &filtered, true).unwrap()), ["Alpha book"]);
    conn.execute(include_str!("sql/subject_compaction_tests/filtered_projection_invalidates_without_rebuilding_unfiltered_tree_for_progress_update_3.sql"), []).unwrap();
    assert_eq!(titles(&compacted_subject_contents(&conn, &filtered, true).unwrap()), ["Alpha book", "Beta book"]);
    conn.execute(include_str!("sql/subject_compaction_tests/compaction_reverses_when_a_second_book_or_subject_appears_update_2.sql"), []).unwrap();
    assert_eq!(titles(&compacted_subject_contents(&conn, &filtered, true).unwrap()), ["Alpha book"]);
}

#[test]
fn filtered_projection_never_caches_rolled_back_state() {
    let conn = direct_fixture();
    let mut filtered = query("World / U.S.");
    filtered.languages = vec!["sv".into()];
    assert_eq!(titles(&compacted_subject_contents(&conn, &filtered, false).unwrap()), ["Beta book"]);
    conn.execute_batch("BEGIN; UPDATE book_language SET language_tag='sv' WHERE book_row_id=(SELECT row_id FROM book WHERE title='Gamma book');").unwrap();
    assert_eq!(titles(&compacted_subject_contents(&conn, &filtered, false).unwrap()), ["Beta book", "Gamma book"]);
    conn.execute_batch(include_str!("sql/subject_compaction_tests/filtered_projection_never_caches_rolled_back_state_rollback.sql")).unwrap();
    assert_eq!(titles(&compacted_subject_contents(&conn, &filtered, false).unwrap()), ["Beta book"]);
}

#[test]
fn stored_projection_survives_reopening_without_rebuild() {
    let conn = fixture();
    let expected = browse(&conn, "World / U.S.");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("library.db");
    conn.execute("VACUUM main INTO ?1", [path.to_str().unwrap()]).unwrap();
    drop(conn);
    let reopened = Connection::open(path).unwrap();
    init_db(&reopened).unwrap();
    reopened.pragma_update(None, "query_only", true).unwrap();
    assert_eq!(browse(&reopened, "World / U.S."), expected);
    assert_eq!(reopened.total_changes(), 0);
}

#[test]
fn scan_bookkeeping_preserves_the_projection_revision() {
    let conn = fixture();
    browse(&conn, "World / U.S.");
    let state = || conn.query_row(include_str!("sql/subject_compaction_tests/scan_bookkeeping_preserves_the_projection_revision_select.sql"), [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))).unwrap();
    let before = state();
    conn.execute(include_str!("sql/subject_compaction_tests/scan_bookkeeping_preserves_the_projection_revision_update.sql"), []).unwrap();
    assert_eq!(state(), before);
    conn.execute(include_str!("sql/subject_compaction_tests/scan_bookkeeping_preserves_the_projection_revision_update_2.sql"), []).unwrap();
    assert_eq!(state(), before);
}

#[test]
fn grouped_browse_retains_recursive_compaction_and_promoted_books() {
    let conn = fixture();
    for location in ["World / U.S.", "World / U.S. / A", "World / U.S. / A / Leaf"] {
        let request = query(location);
        let (contents, groups) = grouped_subject_contents(&conn, &request, false).unwrap();
        assert_eq!(contents, compacted_subject_contents(&conn, &request, false).unwrap());
        assert!(groups.is_empty());
    }
    assert_eq!(children(&grouped_subject_contents(&conn, &query("World / U.S."), false).unwrap().0), ["World / U.S. / A", "World / U.S. / B", "World / U.S. / C"]);
}

#[test]
fn grouping_preserves_compaction_search_filters_and_sort() {
    let conn = fixture();
    conn.execute(include_str!("sql/subject_compaction_tests/grouping_preserves_compaction_search_filters_and_sort_insert.sql"), []).unwrap();
    for location in ["World / U.S.", "World / U.S. / A"] {
        for downloaded_only in [false, true] {
            for option in 0..6 {
                let mut request = query(location);
                match option {
                    1 => request.file_types = vec![LibraryFileTypeFilter::Book],
                    2 => request.chip_sort = BrowseChipSort::BookCount,
                    3 => request.search = "leaf".into(),
                    4 => request.include_direct_child_books = true,
                    5 => request.languages = vec!["sv".into()],
                    _ => {}
                }
                let (contents, groups) = grouped_subject_contents(&conn, &request, downloaded_only).unwrap();
                assert_eq!(contents, compacted_subject_contents(&conn, &request, downloaded_only).unwrap());
                assert!(groups.is_empty());
            }
        }
    }
}

#[test]
fn subject_grouping_does_not_flatten_the_hierarchy() {
    let conn = fixture();
    conn.execute_batch(include_str!("sql/subject_compaction_tests/headings_expand_one_level_after_singleton_branches_are_compacted_delete.sql")).unwrap();
    for (location, expected) in [
        ("", vec!["Domain"]),
        ("Domain", vec!["Domain / Humanities", "Domain / Science"]),
        ("Domain / Humanities", vec!["Domain / Humanities / Philosophy", "Domain / Humanities / Wrapper"]),
        ("Domain / Humanities / Wrapper", vec!["Domain / Humanities / Wrapper / History"]),
        ("Domain / Humanities / Wrapper / History", vec!["Domain / Humanities / Wrapper / History / Ancient", "Domain / Humanities / Wrapper / History / Medieval"]),
    ] {
        for filtered in [false, true] {
            let mut request = query(location);
            request.hide_finished = filtered;
            let (contents, groups) = grouped_subject_contents(&conn, &request, false).unwrap();
            assert_eq!(children(&contents), expected);
            assert!(contents.books.is_empty());
            assert!(groups.is_empty(), "subject headings must not replace parents with grandchildren");
        }
    }
}

#[test]
fn compaction_keeps_books_out_of_the_root_and_its_immediate_children() {
    let conn = fixture();
    for root in ["", ROOT_SUBJECT_PATH] {
        if !root.is_empty() {
            conn.execute(include_str!("sql/subject_compaction_tests/compaction_keeps_books_out_of_the_root_and_its_immediate_children_update_2.sql"), []).unwrap();
            conn.execute(include_str!("sql/subject_compaction_tests/compaction_keeps_books_out_of_the_root_and_its_immediate_children_insert.sql"), []).unwrap();
            conn.execute(include_str!("sql/subject_compaction_tests/compaction_keeps_books_out_of_the_root_and_its_immediate_children_update.sql"), []).unwrap();
        }
        let top = if root.is_empty() { "World" } else { "Subject / World" };
        for location in [root, top] {
            for downloaded_only in [false, true] {
                let mut request = query(location);
                for hide_finished in [false, true] {
                    request.hide_finished = hide_finished;
                    let plain = compacted_subject_contents(&conn, &request, downloaded_only).unwrap();
                    let (grouped, _) = grouped_subject_contents(&conn, &request, downloaded_only).unwrap();
                    assert!(plain.books.is_empty(), "{location} must not receive promoted books");
                    assert!(grouped.books.is_empty());
                    assert!(!plain.children.is_empty());
                    assert!(!grouped.children.is_empty());
                }
            }
        }
        assert!(browse(&conn, &format!("{top} / U.S.")).books.is_empty());
        assert_eq!(browse(&conn, &format!("{top} / U.S.")).children.len(), 3);
    }
    let mut search = query(ROOT_SUBJECT_PATH);
    search.search = "branch".into();
    let (matches, _) = grouped_subject_contents(&conn, &search, false).unwrap();
    assert!(matches.books.is_empty(), "matching subject chips must not compact into books at the root");
    assert!(!matches.children.is_empty());
    search.search = "alpha".into();
    assert_eq!(titles(&grouped_subject_contents(&conn, &search, false).unwrap().0), ["Alpha book"], "explicit book search still returns matching books");
    // Actual assignments at protected levels remain visible.
    conn.execute(include_str!("sql/subject_compaction_tests/compaction_keeps_books_out_of_the_root_and_its_immediate_children_insert_2.sql"), []).unwrap();
    assert_eq!(titles(&browse(&conn, "Subject / World")), ["Alpha book"]);
    assert!(browse(&conn, ROOT_SUBJECT_PATH).books.is_empty());
    conn.execute(include_str!("sql/subject_compaction_tests/compaction_keeps_books_out_of_the_root_and_its_immediate_children_insert_3.sql"), []).unwrap();
    assert_eq!(titles(&browse(&conn, ROOT_SUBJECT_PATH)), ["Beta book"]);
}

#[test]
fn ungrouped_chips_precede_all_groups_with_stable_order_and_ranges() {
    let row = |id: &str| library_model::BrowseRow { id: id.into(), name: id.into(), path: None, book_count: 2, downloaded_book_count: 1 };
    let mut chips = vec![row("Group B"), row("Standalone Z"), row("Group A"), row("Empty group"), row("Standalone A")];
    let replacements = std::collections::HashMap::from([("Group B".into(), vec![row("B2"), row("B1")]), ("Group A".into(), vec![row("A1")]), ("Empty group".into(), vec![])]);
    let groups = crate::browse::expand_chip_groups(&mut chips, replacements);
    assert_eq!(chips.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(), ["Standalone Z", "Empty group", "Standalone A", "B2", "B1", "A1"]);
    assert_eq!(groups.iter().map(|group| (group.parent.id.as_str(), group.start, group.end)).collect::<Vec<_>>(), [("Group B", 3, 5), ("Group A", 5, 6)]);
}

#[test]
fn folder_moves_and_duplicate_copies_preserve_subject_tree() {
    let mut conn = direct_fixture();
    let folder = crate::placements::create_directory(&mut conn, &sync_common::ROOT_DIR_ID, &"Moved".to_owned()).unwrap();
    let hash = ContentHash::new(&format!("{:064x}", 901));
    let before = browse(&conn, "World / U.S.");
    let tree = |conn: &Connection| conn.query_row(include_str!("sql/subject_compaction_tests/folder_moves_and_duplicate_copies_preserve_subject_tree_select.sql"), [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).unwrap();
    let revision = tree(&conn);
    for (source, destination, moving) in [(sync_common::ROOT_DIR_ID, folder.id, true), (folder.id, sync_common::ROOT_DIR_ID, false)] {
        let tx = conn.transaction().unwrap();
        crate::placements::transfer_book_placement_record(&tx, &hash, &source, &destination, moving).unwrap();
        tx.commit().unwrap();
        assert_eq!(tree(&conn), revision, "moves and copies must reuse the subject tree");
        assert_eq!(browse(&conn, "World / U.S."), before);
    }
    conn.execute(include_str!("sql/subject_compaction_tests/folder_moves_and_duplicate_copies_preserve_subject_tree_delete.sql"), rusqlite::params![folder.id.to_string(), hash.as_str()]).unwrap();
    assert_eq!(tree(&conn), revision, "removing a duplicate must preserve the tree");
    conn.execute(include_str!("sql/subject_compaction_tests/folder_moves_and_duplicate_copies_preserve_subject_tree_update.sql"), [hash.as_str()]).unwrap();
    assert_eq!(tree(&conn), revision, "download availability must not invalidate the hierarchy");
    browse(&conn, "World / U.S.");
    let downloaded_revision = tree(&conn);
    conn.execute(include_str!("sql/subject_compaction_tests/folder_moves_and_duplicate_copies_preserve_subject_tree_update_2.sql"), [hash.as_str()]).unwrap();
    assert!(tree(&conn).0 > downloaded_revision.0, "removing the last live placement must refresh membership");
    assert!(!titles(&browse(&conn, "World / U.S.")).contains(&"Alpha book"));
    let removed_revision = tree(&conn);
    conn.execute(include_str!("sql/subject_compaction_tests/folder_moves_and_duplicate_copies_preserve_subject_tree_update_3.sql"), [hash.as_str()]).unwrap();
    assert!(tree(&conn).0 > removed_revision.0, "restoring the first placement must refresh membership");
    assert!(titles(&browse(&conn, "World / U.S.")).contains(&"Alpha book"));
}

#[test]
fn downloads_refresh_counts_without_writing_the_subject_hierarchy() {
    let conn = fixture();
    let q = query("World");
    let before = grouped_subject_contents(&conn, &q, false).unwrap();
    assert!(before.0.children.iter().any(|row| row.downloaded_book_count > 0));
    conn.execute_batch(include_str!("sql/subject_compaction_tests/protect_download_hierarchy.sql")).unwrap();
    conn.execute(include_str!("sql/subject_compaction_tests/downloads_refresh_counts_without_writing_the_subject_hierarchy_2.sql"), []).unwrap();
    let after = grouped_subject_contents(&conn, &q, false).unwrap();
    assert_eq!(children(&before.0), children(&after.0));
    assert!(after.0.children.iter().all(|row| row.downloaded_book_count == 0));
    conn.execute(include_str!("sql/subject_compaction_tests/downloads_refresh_counts_without_writing_the_subject_hierarchy_3.sql"), []).unwrap();
    let restored = grouped_subject_contents(&conn, &q, false).unwrap();
    assert_eq!(children(&before.0), children(&restored.0));
    assert!(restored.0.children.iter().all(|row| row.downloaded_book_count == row.book_count));
}

#[test]
fn membership_updates_only_affected_branches_and_matches_a_fresh_build() {
    let conn = fixture();
    browse(&conn, "World");
    let paths = ["", "World", "World / U.S.", "World / U.S. / A", "World / U.S. / A / Leaf", "World / U.S. / B", "World / U.S. / C"];
    for change in [
        include_str!("sql/subject_compaction_tests/membership_updates_only_affected_branches_and_matches_a_fresh_build_fixture_1.sql"),
        include_str!("sql/subject_compaction_tests/membership_updates_only_affected_branches_and_matches_a_fresh_build_1.sql"),
        include_str!("sql/subject_compaction_tests/membership_updates_only_affected_branches_and_matches_a_fresh_build_2.sql"),
        include_str!("sql/subject_compaction_tests/membership_updates_only_affected_branches_and_matches_a_fresh_build_3.sql"),
        include_str!("sql/subject_compaction_tests/membership_updates_only_affected_branches_and_matches_a_fresh_build_4.sql"),
        include_str!("sql/subject_compaction_tests/membership_updates_only_affected_branches_and_matches_a_fresh_build_5.sql"),
        include_str!("sql/subject_compaction_tests/membership_updates_only_affected_branches_and_matches_a_fresh_build_6.sql"),
        include_str!("sql/subject_compaction_tests/membership_updates_only_affected_branches_and_matches_a_fresh_build_7.sql"),
    ] {
        // A rebuild of the whole hierarchy would hit these unrelated branches.
        conn.execute_batch(include_str!("sql/subject_compaction_tests/protect_unrelated_subject_branches.sql")).unwrap();
        conn.execute_batch(change).unwrap();
        let incremental = paths.iter().map(|path| grouped_subject_contents(&conn, &query(path), false).unwrap()).collect::<Vec<_>>();
        for (path, expected) in paths.iter().zip(&incremental) {
            let mut reference = query(path);
            // Whitespace selects the independent in-memory implementation,
            // but normalizes to an empty search with the same book selection.
            reference.search = " ".into();
            assert_eq!(&grouped_subject_contents(&conn, &reference, false).unwrap(), expected);
        }
        conn.execute_batch(include_str!("sql/subject_compaction_tests/membership_updates_only_affected_branches_and_matches_a_fresh_build_fixture_2.sql")).unwrap();
        let rebuilt = paths.iter().map(|path| grouped_subject_contents(&conn, &query(path), false).unwrap()).collect::<Vec<_>>();
        assert_eq!(incremental, rebuilt, "incremental and initial construction must agree after {change}");
    }
}

#[test]
fn download_flags_do_not_invalidate_language_filtered_subjects() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    let conn = direct_fixture();
    let mut q = query("World / U.S.");
    q.languages = vec!["en".into()];
    let before = grouped_subject_contents(&conn, &q, false).unwrap();
    assert!(before.0.books.iter().any(|book| book.title == "Alpha book" && book.downloaded));
    let revisions = |conn: &Connection| {
        conn.query_row(include_str!("sql/subject_compaction_tests/download_flags_do_not_invalidate_language_filtered_subjects_1.sql"), [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))).unwrap()
    };
    let saved = revisions(&conn);
    assert!(!conn.query_row(include_str!("sql/subject_compaction_tests/download_flags_do_not_invalidate_language_filtered_subjects_3.sql"), [], |r| r.get::<_, bool>(0)).unwrap());
    assert!(!conn.query_row(include_str!("sql/subject_compaction_tests/download_flags_do_not_invalidate_language_filtered_subjects_4.sql"), [], |r| r.get::<_, bool>(0)).unwrap());
    conn.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Insert { table_name } | AuthAction::Delete { table_name } | AuthAction::Update { table_name, .. } if table_name.starts_with("subject_") || table_name == "library_subject" => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    conn.execute(include_str!("sql/subject_compaction_tests/download_flags_do_not_invalidate_language_filtered_subjects_5.sql"), []).unwrap();
    conn.execute(include_str!("sql/subject_compaction_tests/download_flags_do_not_invalidate_language_filtered_subjects_6.sql"), []).unwrap();
    assert_eq!(revisions(&conn), saved, "download state must not invalidate subject data");
    // Any attempt to build the filtered graph again would read this table.
    conn.authorizer(Some(|context: AuthContext<'_>| match context.action {
        AuthAction::Read { table_name: "library_subject", .. } => Authorization::Deny,
        _ => Authorization::Allow,
    }))
    .unwrap();
    let after = grouped_subject_contents(&conn, &q, false).unwrap();
    assert_eq!(titles(&before.0), titles(&after.0));
    assert!(after.0.books.iter().all(|book| !book.downloaded));
    assert!(after.0.books.iter().any(|book| book.title == "Gamma book" && book.download_requested));
    conn.authorizer(None::<fn(AuthContext<'_>) -> Authorization>).unwrap();
    assert!(grouped_subject_contents(&conn, &q, true).unwrap().0.books.is_empty());
    conn.execute(include_str!("sql/subject_compaction_tests/download_flags_do_not_invalidate_language_filtered_subjects_7.sql"), []).unwrap();
    assert_eq!(titles(&grouped_subject_contents(&conn, &q, true).unwrap().0), ["Alpha book"]);
}
