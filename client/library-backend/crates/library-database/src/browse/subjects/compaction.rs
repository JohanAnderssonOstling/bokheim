//! Compaction uses all distinct library books; cached routes retain their original identity.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Node {
    label: String,
    parent: i64,
    children: BTreeSet<i64>,
    members: BTreeSet<i64>,
    direct: BTreeSet<i64>,
}

#[derive(Default)]
struct Contents {
    books: BTreeSet<i64>,
    subjects: BTreeSet<i64>,
}

fn contents(route: i64, nodes: &BTreeMap<i64, Node>) -> Contents {
    let Some(node) = nodes.get(&route) else { return Contents::default() };
    let mut result = Contents { books: node.direct.clone(), ..Default::default() };
    for child in &node.children {
        let branch = &nodes[child];
        if branch.members.is_empty() {
            continue;
        }
        // First flatten small subtrees, regardless of their internal shape.
        if branch.members.len() <= 3 {
            result.books.extend(&branch.members);
            continue;
        }
        let compacted = contents(*child, nodes);
        // Then bypass empty single-child chains. Promoted books count as books.
        if compacted.books.is_empty() && compacted.subjects.len() == 1 {
            result.subjects.extend(compacted.subjects);
        } else {
            result.subjects.insert(*child);
        }
    }
    result
}

struct MatchingBooks {
    books: BTreeSet<i64>,
    downloaded: BTreeSet<i64>,
}

fn compacted_selection(location: i64, nodes: &BTreeMap<i64, Node>, include_direct_child_books: bool) -> Contents {
    // Structure is determined by all distinct books, independently of filters.
    let mut shown = contents(location, nodes);
    if include_direct_child_books {
        if let Some(root) = nodes.get(&location) {
            for child in &root.children {
                shown.books.extend(&nodes[child].direct);
            }
        }
    }

    shown
}

impl Database {
    pub(super) fn compact_subject_contents(&self, location: i64, options: &crate::browse::BrowseOptions) -> Result<library_model::BrowseContents, DatabaseError> {
        let nodes = self.load_subject_tree(location)?;
        let mut shown = compacted_selection(location, &nodes, options.include_direct_child_books);
        let matching = self.matching_subject_books(location, &nodes, options)?;
        shown.books.retain(|book| matching.books.contains(book));
        let children = self.displayed_subject_rows(location, &nodes, &shown, &matching, options)?;
        let books = self.displayed_subject_cards(&shown.books, options)?;
        Ok(library_model::BrowseContents { children, books })
    }

    fn load_subject_tree(&self, location: i64) -> Result<BTreeMap<i64, Node>, DatabaseError> {
        let mut nodes = BTreeMap::<i64, Node>::new();
        self.connection.subject_cached_members(location, |row| {
            let route = row.get(0)?;
            let node = nodes.entry(route).or_default();
            node.parent = row.get(1)?;
            node.label = row.get(2)?;
            let book = row.get(3)?;
            node.members.insert(book);
            if row.get::<_, bool>(4)? {
                node.direct.insert(book);
            }
            Ok(())
        })?;
        let edges = nodes.iter().map(|(route, node)| (*route, node.parent)).collect::<Vec<_>>();
        for (route, parent) in edges {
            if route != parent {
                if let Some(node) = nodes.get_mut(&parent) {
                    node.children.insert(route);
                }
            }
        }

        Ok(nodes)
    }

    fn matching_subject_books(&self, location: i64, nodes: &BTreeMap<i64, Node>, options: &crate::browse::BrowseOptions) -> Result<MatchingBooks, DatabaseError> {
        let mut matching = BTreeSet::new();
        let mut downloaded = BTreeSet::new();
        let mut search_matches = BTreeSet::new();
        self.connection.subject_matching_books(location, i32::from(options.downloaded_only), options.file_types, &options.languages, &options.search, |row| {
            // Match card decoding's f32 precision at the finished threshold.
            if options.hide_finished && row.get::<_, f32>(3)? >= library_model::FINISHED_PROGRESS_THRESHOLD {
                return Ok(());
            }
            let book = row.get(0)?;
            matching.insert(book);
            if row.get::<_, bool>(1)? {
                downloaded.insert(book);
            }
            if row.get::<_, bool>(2)? {
                search_matches.insert(book);
            }
            Ok(())
        })?;
        if !options.search.is_empty() {
            for (route, node) in nodes {
                if *route != location && book_model::normalize_search_text(&node.label).contains(&options.search) {
                    search_matches.extend(&node.members);
                }
            }
            matching.retain(|book| search_matches.contains(book));
        }
        downloaded.retain(|book| matching.contains(book));
        Ok(MatchingBooks { books: matching, downloaded })
    }

    fn displayed_subject_rows(&self, location: i64, nodes: &BTreeMap<i64, Node>, shown: &Contents, matching: &MatchingBooks, options: &crate::browse::BrowseOptions) -> Result<Vec<BrowseRow>, DatabaseError> {
        let mut children = Vec::new();
        for &route in &shown.subjects {
            let node = &nodes[&route];
            let book_count = node.members.intersection(&matching.books).count();
            if book_count == 0 {
                continue;
            }
            let id = self.subject_route_path(route)?.ok_or_else(|| DatabaseError::message("missing subject route"))?;
            let path = if node.parent != location { self.subject_route_path(node.parent)? } else { None };
            children.push(BrowseRow { id, name: node.label.clone(), path, book_count, downloaded_book_count: node.members.intersection(&matching.downloaded).count() });
        }
        children.sort_by(|left, right| {
            let counts = if options.chip_sort == BrowseChipSort::BookCount { right.book_count.cmp(&left.book_count) } else { std::cmp::Ordering::Equal };
            counts.then_with(|| book_model::normalize_search_text(&left.name).cmp(&book_model::normalize_search_text(&right.name))).then_with(|| left.name.cmp(&right.name)).then_with(|| left.id.cmp(&right.id))
        });
        Ok(children)
    }

    fn displayed_subject_cards(&self, selected: &BTreeSet<i64>, options: &crate::browse::BrowseOptions) -> Result<Vec<library_model::BookCardRow>, DatabaseError> {
        let mut books = Vec::new();
        if !selected.is_empty() {
            let ids = serde_json::to_string(selected).map_err(DatabaseError::operation)?;
            self.connection.subject_displayed_cards(&ids, book_sort_query_value(options.book_sort), |row| {
                books.push(read_book_card_row(row)?);
                Ok(())
            })?;
        }
        Ok(books)
    }
}

#[cfg(test)]
mod reference;

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(book_count: i64) -> (Database, Vec<String>) {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.execute("UPDATE sync_metadata SET change_origin='remote'", []).unwrap();
        // Include multi-parent concepts and paths at several depths.
        let routes = db
            .prepare("SELECT concept_id,path FROM curated.unified_concept_paths WHERE concept_id>0 AND parent_path!='Subject' ORDER BY concept_id,path LIMIT 20")
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        let tx = db.unchecked_transaction().unwrap();
        for book in 1..=book_count {
            tx.execute(
                "INSERT INTO book(content_hash,title,subtitle,format,read_progress,added_at) VALUES(?1,?2,'Écology',?3,?4,?5)",
                rusqlite::params![format!("{book:064x}"), format!("Book {book}"), ["epub", "pdf", "m4b"][book as usize % 3], if book % 5 == 0 { 100. } else { 25. }, book * 100],
            )
            .unwrap();
            tx.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES(?1,?2,?4,'',?3)", rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), book, book % 2, format!("book{book}")]).unwrap();
            tx.execute("INSERT INTO book_language VALUES(?1,0,?2)", rusqlite::params![book, if book % 3 == 0 { "sv" } else { "en-US" }]).unwrap();
            tx.execute("INSERT INTO book_unified_concept VALUES(?1,?2,1)", rusqlite::params![book, routes[book as usize % routes.len()].0]).unwrap();
            if book % 7 == 0 {
                tx.execute("INSERT OR IGNORE INTO book_unified_concept VALUES(?1,?2,1)", rusqlite::params![book, routes[0].0]).unwrap();
            }
            tx.execute("INSERT INTO book_description VALUES(?1,?2)", rusqlite::params![book, "Description ".repeat(200)]).unwrap();
        }
        tx.commit().unwrap();
        let paths = routes.into_iter().map(|(_, path)| path).collect();
        (db, paths)
    }

    fn query(location: &str) -> library_model::LibraryBrowseQuery {
        library_model::LibraryBrowseQuery {
            location: location.into(),
            search: String::new(),
            file_types: vec![],
            languages: vec![],
            chip_sort: BrowseChipSort::Alphabetical,
            book_sort: BrowseBookSort::Alphabetical,
            hide_finished: false,
            include_direct_child_books: false,
        }
    }

    #[test]
    fn numeric_compaction_matches_previous_path_implementation() {
        let (db, paths) = fixture(48);
        db.execute("UPDATE book SET title=NULL WHERE row_id=48", []).unwrap();
        db.execute("UPDATE book SET read_progress=97.9999999 WHERE row_id=1", []).unwrap();
        for location in ["Subject", "", paths[0].as_str(), paths[9].as_str(), "Missing subject"] {
            for search in ["", "book 1", "ecology", "untitled", paths[0].rsplit(" / ").next().unwrap(), "missing"] {
                for case in 0..8 {
                    let mut query = query(location);
                    query.search = search.into();
                    query.hide_finished = case & 1 != 0;
                    query.include_direct_child_books = case & 2 != 0;
                    if case & 4 != 0 {
                        query.languages = vec!["EN_us".into(), "en-GB".into()];
                        query.file_types = vec![LibraryFileTypeFilter::Book, LibraryFileTypeFilter::Pdf];
                        query.chip_sort = BrowseChipSort::BookCount;
                        query.book_sort = BrowseBookSort::RecentlyRead;
                    }
                    let downloaded = case & 2 != 0;
                    let expected = reference::browse(&db, &query, downloaded).unwrap();
                    assert_eq!(db.browse_subject_contents(&query, downloaded).unwrap(), expected, "location={location} search={search} case={case}");
                }
            }
        }
    }

    #[test]
    #[ignore = "release performance measurement"]
    fn subject_compaction_performance() {
        use std::time::Instant;
        let (db, paths) = fixture(4000);
        for location in ["Subject", paths[0].as_str()] {
            let query = query(location);
            let expected = reference::browse(&db, &query, false).unwrap();
            assert_eq!(db.browse_subject_contents(&query, false).unwrap(), expected);
            let mut previous = Vec::new();
            let mut current = Vec::new();
            for _ in 0..7 {
                let start = Instant::now();
                std::hint::black_box(reference::browse(&db, &query, false).unwrap());
                previous.push(start.elapsed());
                let start = Instant::now();
                std::hint::black_box(db.browse_subject_contents(&query, false).unwrap());
                current.push(start.elapsed());
            }
            previous.sort();
            current.sort();
            eprintln!("subject {location}: previous {:?}, current {:?}, {:.2}x", previous[3], current[3], previous[3].as_secs_f64() / current[3].as_secs_f64());
        }
    }

    fn id(name: &str) -> i64 {
        name.bytes().fold(0, |id, byte| id * 128 + i64::from(byte))
    }

    fn tree(entries: &[(&str, &str, &[i64])]) -> BTreeMap<i64, Node> {
        let mut nodes = BTreeMap::new();
        for (path, parent, books) in entries {
            nodes.insert(id(path), Node { parent: id(parent), direct: books.iter().copied().collect(), members: books.iter().copied().collect(), ..Default::default() });
        }
        // Fixtures list parents before descendants.
        for (path, parent, _) in entries.iter().rev() {
            if path != parent {
                let members = nodes[&id(path)].members.clone();
                let node = nodes.get_mut(&id(parent)).unwrap();
                node.children.insert(id(path));
                node.members.extend(members);
            }
        }
        nodes
    }

    #[test]
    fn three_books_flatten_but_four_keep_a_subject() {
        let nodes = tree(&[("root", "root", &[]), ("small", "root", &[0, 1, 2]), ("large", "root", &[3, 4, 5, 6])]);
        let shown = contents(id("root"), &nodes);
        assert_eq!(shown.books, BTreeSet::from([0, 1, 2]));
        assert_eq!(shown.subjects, BTreeSet::from([id("large")]));
    }

    #[test]
    fn empty_single_child_chains_collapse_to_the_leaf() {
        let nodes = tree(&[("root", "root", &[]), ("a", "root", &[]), ("b", "a", &[]), ("leaf", "b", &[0, 1, 2, 3])]);
        let shown = contents(id("root"), &nodes);
        assert!(shown.books.is_empty());
        assert_eq!(shown.subjects, BTreeSet::from([id("leaf")]));
        assert_eq!(contents(id("leaf"), &nodes).books.len(), 4);
    }

    #[test]
    fn small_subtrees_flatten_before_single_child_checks() {
        let nodes = tree(&[("root", "root", &[]), ("branch", "root", &[]), ("small", "branch", &[0]), ("large", "branch", &[1, 2, 3, 4])]);
        let shown = contents(id("root"), &nodes);
        // The promoted small book prevents bypassing branch to large.
        assert_eq!(shown.subjects, BTreeSet::from([id("branch")]));
        assert_eq!(contents(id("branch"), &nodes).books, BTreeSet::from([0]));
    }

    #[test]
    fn shared_books_are_counted_and_promoted_once() {
        let nodes = tree(&[("root", "root", &[]), ("branch", "root", &[]), ("a", "branch", &[0, 1, 2]), ("b", "branch", &[0, 1, 2])]);
        let shown = contents(id("root"), &nodes);
        assert_eq!(shown.books, BTreeSet::from([0, 1, 2]));
        assert!(shown.subjects.is_empty());
    }

    #[test]
    fn direct_books_and_multiple_children_preserve_branches() {
        let nodes = tree(&[("root", "root", &[]), ("direct", "root", &[0]), ("leaf", "direct", &[1, 2, 3, 4]), ("fork", "root", &[]), ("left", "fork", &[5, 6, 7, 8]), ("right", "fork", &[9, 10, 11, 12])]);
        assert_eq!(contents(id("root"), &nodes).subjects, BTreeSet::from([id("direct"), id("fork")]));
    }
}
