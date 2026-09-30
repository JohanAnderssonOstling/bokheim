//! Subject browsing: taxonomy.

use crate::browse::books::MatchingBooks;
use crate::browse::{BrowseOptions, BrowseRow, SubjectsSql};
use crate::{Database, DatabaseError};
use library_model::{BookCardRow, BrowseChipSort};
use std::collections::{BTreeMap, BTreeSet};

mod tree;

#[derive(Default)]
struct SubjectNode {
    label: String,
    parent: i64,
    children: BTreeSet<i64>,
    members: BTreeSet<i64>,
    direct: BTreeSet<i64>,
}

impl Database {
    /// Complete subject page from one read snapshot.
    pub fn browse_subject(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<super::LibraryBrowseData, DatabaseError> {
        self.with_subject_browse(query, downloaded_only, |route, nodes, matching, options| {
            let contents = self.subject_contents(route, nodes, matching, options)?;
            let mut sections = Vec::new();
            if options.search.is_empty() {
                for parent in &contents.children {
                    let child_route = parent.id.parse::<i64>().map_err(DatabaseError::operation)?;
                    let child_contents = self.subject_contents(child_route, nodes, matching, options)?;
                    if child_contents.books.is_empty() && child_contents.children.is_empty() {
                        continue;
                    }
                    sections.push(library_model::BrowseSection { parent: parent.clone(), children: child_contents.children, books: child_contents.books });
                }
            }
            let (language_counts, format_counts) = self.library_facets(options)?;
            let path = subject_browse_path(route, nodes);
            Ok(super::LibraryBrowseData { contents, sections, path, language_counts, format_counts })
        })
    }

    /// Show immediate subject routes and books assigned directly to this route.
    pub fn browse_subject_contents(&self, query: &library_model::LibraryBrowseQuery, downloaded_only: bool) -> Result<library_model::BrowseContents, DatabaseError> {
        self.with_subject_browse(query, downloaded_only, |route, nodes, matching, options| self.subject_contents(route, nodes, matching, options))
    }

    /// One tree and filtering result serve the selected node and its sections.
    fn with_subject_browse<T>(
        &self,
        query: &library_model::LibraryBrowseQuery,
        downloaded_only: bool,
        read: impl FnOnce(i64, &BTreeMap<i64, SubjectNode>, &MatchingBooks, &BrowseOptions) -> Result<T, DatabaseError>,
    ) -> Result<T, DatabaseError> {
        let route = query.location.parse::<i64>().map_err(DatabaseError::operation)?;
        let options = BrowseOptions::from_query(query, downloaded_only);
        self.with_browse_snapshot(|| {
            let nodes = self.load_subject_tree()?;
            let candidates = nodes.get(&route).map(|node| &node.members);
            let matching = self.matching_books(candidates.unwrap_or(&BTreeSet::new()), &options)?;
            read(route, &nodes, &matching, &options)
        })
    }

    /// Node contents shared by graph expansion, downloads, and card sections.
    fn subject_contents(&self, location: i64, nodes: &BTreeMap<i64, SubjectNode>, matching: &MatchingBooks, options: &BrowseOptions) -> Result<library_model::BrowseContents, DatabaseError> {
        let Some(root) = nodes.get(&location) else { return Ok(library_model::BrowseContents::default()) };
        let searched = !options.search.is_empty();
        let mut selected_books = if searched { root.members.intersection(&matching.text_matches).copied().collect() } else { root.direct.clone() };
        if !searched && options.include_direct_child_books {
            for child in &root.children {
                selected_books.extend(&nodes[child].direct);
            }
        }
        selected_books.retain(|book| matching.books.contains(book));

        let routes = if searched { tree::subtree_routes(nodes, location) } else { root.children.clone() };
        let mut children = Vec::new();
        for route in routes {
            let node = &nodes[&route];
            if searched && (route == location || !book_model::normalize_search_text(&node.label).contains(&options.search)) {
                continue;
            }
            if let Some(row) = Self::subject_child_row(route, node, matching, || if searched { subject_path_below(location, route, nodes) } else { None }) {
                children.push(row);
            }
        }
        sort_subject_rows(&mut children, options.chip_sort);
        let books = self.subject_cards(&selected_books, options)?;
        Ok(library_model::BrowseContents { children, books })
    }

    fn subject_child_row(route: i64, node: &SubjectNode, matching: &MatchingBooks, path: impl FnOnce() -> Option<String>) -> Option<BrowseRow> {
        let book_count = node.members.intersection(&matching.books).count();
        if book_count == 0 {
            return None;
        }
        let id = route.to_string();
        Some(BrowseRow { id, name: node.label.clone(), path: path(), book_count, downloaded_book_count: node.members.intersection(&matching.downloaded).count() })
    }

    fn subject_cards(&self, selected_books: &BTreeSet<i64>, options: &super::BrowseOptions) -> Result<Vec<BookCardRow>, DatabaseError> {
        self.load_cards(&self.ordered_books(selected_books, options)?)
    }
}

fn sort_subject_rows(rows: &mut [BrowseRow], chip_sort: BrowseChipSort) {
    rows.sort_by(|left, right| {
        let counts = if chip_sort == BrowseChipSort::BookCount { right.book_count.cmp(&left.book_count) } else { std::cmp::Ordering::Equal };
        counts.then_with(|| book_model::normalize_search_text(&left.name).cmp(&book_model::normalize_search_text(&right.name))).then_with(|| left.name.cmp(&right.name)).then_with(|| left.id.cmp(&right.id))
    });
}

/// The labels between `location` and `route`, outermost first, as a searched
/// subject's chip states them — the way a searched folder carries the path to
/// its parent. `None` for a direct child, which needs no placing.
fn subject_path_below(location: i64, route: i64, nodes: &BTreeMap<i64, SubjectNode>) -> Option<String> {
    let mut labels = Vec::new();
    let mut current = nodes.get(&route)?.parent;
    while current != location {
        let Some(node) = nodes.get(&current) else { break };
        labels.push(node.label.as_str());
        if node.parent == current {
            break;
        }
        current = node.parent;
    }
    labels.reverse();
    (!labels.is_empty()).then(|| labels.join(" \u{203a} "))
}

/// Breadcrumbs use displayed parents, while every location remains a route ID.
fn subject_browse_path(mut route: i64, nodes: &BTreeMap<i64, SubjectNode>) -> Vec<library_model::BrowsePathSegment> {
    let mut path = Vec::new();
    while route != library_model::ROOT_SUBJECT_ROUTE_ID {
        let Some(node) = nodes.get(&route) else { return Vec::new() };
        path.push(library_model::BrowsePathSegment { id: route.to_string(), name: node.label.clone() });
        route = node.parent;
    }
    path.reverse();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_cards_navigate_by_id_and_breadcrumbs_follow_compacted_parents() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.connection
            .execute_batch(
                "UPDATE sync_metadata SET change_origin='remote';
            DETACH curated; ATTACH ':memory:' AS curated;
            CREATE TABLE curated.concept(concept_id INTEGER PRIMARY KEY,preferred_label TEXT NOT NULL);
            INSERT INTO curated.concept VALUES(1,'Science'),(2,'Physics'),(3,'Quantum'),(4,'Optics');
            CREATE TABLE curated.unified_concept_route(route_id INTEGER PRIMARY KEY,concept_id INTEGER NOT NULL,parent_route_id INTEGER NOT NULL);
            INSERT INTO curated.unified_concept_route VALUES(1,1,0),(2,2,1),(3,3,2),(4,4,2);",
            )
            .unwrap();
        for id in 1..=6 {
            db.connection.execute("INSERT INTO book(row_id,content_hash,title,search_title,format) VALUES(?1,printf('%064x',?1),printf('Book %d',?1),printf('book %d',?1),1)", [id]).unwrap();
            db.connection.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES(?1,?2,?3,'',1)", rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), id, format!("book{id}.epub")]).unwrap();
            db.connection.execute("INSERT INTO book_unified_concept VALUES(?1,?2,1)", [id, if id <= 3 { 3 } else { 4 }]).unwrap();
        }
        let mut query = library_model::LibraryBrowseQuery {
            location: library_model::ROOT_SUBJECT_ROUTE_ID.to_string(),
            search: String::new(),
            file_types: vec![],
            languages: vec![],
            chip_sort: Default::default(),
            book_sort: Default::default(),
            hide_finished: false,
            include_direct_child_books: false,
        };
        let root = db.browse_subject(&query, false).unwrap();
        assert!(root.path.is_empty());
        assert_eq!(root.contents.children[0].id, "1");
        let science = &root.sections[0];
        assert_eq!(science.parent.id, "1");
        assert_eq!(science.children.iter().map(|child| (child.id.as_str(), child.name.as_str())).collect::<Vec<_>>(), [("4", "Optics"), ("3", "Quantum")]);
        query.location = science.children[1].id.clone();
        let quantum = db.browse_subject(&query, false).unwrap();
        assert_eq!(quantum.contents.books.len(), 3);
        assert_eq!(quantum.path.iter().map(|segment| (segment.id.as_str(), segment.name.as_str())).collect::<Vec<_>>(), [("1", "Science"), ("3", "Quantum")]);
        db.connection
            .execute_batch(
                "UPDATE curated.concept SET preferred_label='1 / science' WHERE concept_id=1;
            UPDATE curated.concept SET preferred_label='Quantum / fields' WHERE concept_id=3;",
            )
            .unwrap();
        let renamed = db.browse_subject(&query, false).unwrap();
        assert_eq!(renamed.contents.books.len(), 3);
        assert_eq!(renamed.path.iter().map(|segment| (segment.id.as_str(), segment.name.as_str())).collect::<Vec<_>>(), [("1", "1 / science"), ("3", "Quantum / fields")]);
        query.location = "0".into();
        query.search = "fields".into();
        assert_eq!(db.browse_subject(&query, false).unwrap().contents.children[0].id, "3");
        query.location = "Science / Physics / Quantum".into();
        assert!(db.browse_subject(&query, false).is_err(), "labels are no longer interpreted as navigation IDs");
        assert!(db.connection.is_autocommit());
    }
}
