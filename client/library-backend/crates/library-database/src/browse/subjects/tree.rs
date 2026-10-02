//! Build and compact the displayed subject tree from current assignments.
pub(crate) use super::*;

impl Database {
    pub(super) fn load_subject_tree(&self) -> Result<BTreeMap<i64, SubjectNode>, DatabaseError> {
        let mut nodes = BTreeMap::from([(0, SubjectNode { label: "Subject".into(), ..Default::default() })]);
        let mut assignments = Vec::new();
        self.connection.subject_assignments(|row| {
            assignments.push((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?));
            Ok(())
        })?;
        let mut lookup = self.connection.prepare_cached(include_str!("../sql/subjects/parent.sql"))?;
        for (book, assigned_route) in assignments {
            let mut current = assigned_route;
            while current != 0 {
                if !nodes.contains_key(&current) {
                    let (parent, label) = lookup.query_row([current], |row| Ok((row.get(0)?, row.get(1)?)))?;
                    nodes.insert(current, SubjectNode { parent, label, ..Default::default() });
                }
                let node = nodes.get_mut(&current).expect("loaded route");
                node.members.insert(book);
                if current == assigned_route {
                    node.direct.insert(book);
                }
                current = node.parent;
            }
            nodes.get_mut(&0).expect("subject root").members.insert(book);
        }
        let edges = nodes.iter().filter(|(route, _)| **route != 0).map(|(&route, node)| (route, node.parent)).collect::<Vec<_>>();
        for (route, parent) in edges {
            nodes.get_mut(&parent).expect("loaded ancestor").children.insert(route);
        }
        let branches = nodes[&0].children.clone();
        for branch in branches {
            if !compact_node(branch, &mut nodes) {
                let members = nodes[&branch].members.clone();
                let root = nodes.get_mut(&0).expect("subject root");
                root.direct.extend(members);
                root.children.remove(&branch);
            }
        }
        // Compacted-away nodes are no longer valid browse locations.
        let visible = subtree_routes(&nodes, 0);
        nodes.retain(|route, _| visible.contains(route));
        Ok(nodes)
    }
}

/// Collapse small leaves, then absorb a sole child of an empty wrapper while
/// keeping the wrapper's label. Membership is independent of page filters.
pub(crate) fn compact_node(route: i64, nodes: &mut BTreeMap<i64, SubjectNode>) -> bool {
    let children = nodes[&route].children.clone();
    for child in children {
        if !compact_node(child, nodes) {
            let members = nodes[&child].members.clone();
            let node = nodes.get_mut(&route).expect("subject route");
            node.direct.extend(members);
            node.children.remove(&child);
        }
    }
    let node = &nodes[&route];
    if node.children.is_empty() && node.members.len() <= 2 {
        return false;
    }
    if node.direct.is_empty() && node.children.len() == 1 {
        let child = *node.children.first().expect("sole child");
        let direct = nodes[&child].direct.clone();
        let grandchildren = nodes[&child].children.clone();
        for grandchild in &grandchildren {
            nodes.get_mut(grandchild).expect("subject grandchild").parent = route;
        }
        let node = nodes.get_mut(&route).expect("subject route");
        node.direct = direct;
        node.children = grandchildren;
    }
    true
}

pub(crate) fn subtree_routes(nodes: &BTreeMap<i64, SubjectNode>, root: i64) -> BTreeSet<i64> {
    let mut routes = BTreeSet::new();
    let mut pending = vec![root];
    while let Some(route) = pending.pop() {
        if let Some(node) = nodes.get(&route) {
            if routes.insert(route) {
                pending.extend(&node.children);
            }
        }
    }
    routes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transactions::WriteOutcome;

    #[test]
    fn read_time_compaction_restores_and_recascades_after_assignment_changes() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        // This fixture owns derived rows directly; do not enqueue canonical edits.

        db.connection
            .execute_batch(
                "DETACH curated; ATTACH ':memory:' AS curated;
            CREATE TABLE curated.taxonomy_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE TABLE curated.concept(concept_id INTEGER PRIMARY KEY,preferred_label TEXT NOT NULL);
            INSERT INTO curated.concept VALUES(1,'Transportation'),(2,'Land Transport'),(3,'Automotive'),(4,'Railroads'),(5,'Marine Transport');
            CREATE TABLE curated.unified_concept_route(route_id INTEGER PRIMARY KEY,concept_id INTEGER NOT NULL,parent_route_id INTEGER NOT NULL);
            INSERT INTO curated.unified_concept_route VALUES(1,1,0),(2,2,1),(3,3,2),(4,4,2),(5,5,1);
        ",
            )
            .unwrap();
        let insert = |first: i64, last: i64| {
            db.with_write_transaction(|tx| {
                for id in first..=last {
                    tx.execute("INSERT INTO book(content_hash,title,format) VALUES(printf('%064x',?1),printf('Book %d',?1),1)", [id])?;
                    tx.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES(?1,?2,?3,'',1)", rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), id, format!("book{id}.epub")])?;
                    let concept = match id {
                        1 | 4 | 5 => 3,
                        2 => 4,
                        3 => 5,
                        _ => unreachable!(),
                    };
                    tx.execute("INSERT INTO book_unified_concept(book_row_id,concept_id,mapper_version) VALUES(?1,?2,1)", [id, concept])?;
                }
                Ok(WriteOutcome::Commit(()))
            })
            .unwrap();
        };
        let owners = || db.load_subject_tree().unwrap().into_iter().flat_map(|(route, node)| node.direct.into_iter().map(move |book| (route, book))).collect::<Vec<_>>();
        let visible = || db.load_subject_tree().unwrap().into_keys().filter(|route| *route != 0).collect::<Vec<_>>();
        insert(1, 2);
        assert_eq!(visible(), Vec::<i64>::new());
        assert_eq!(owners(), vec![(0, 1), (0, 2)]);
        insert(3, 3);
        assert_eq!(visible(), vec![1]);
        assert_eq!(owners(), vec![(1, 1), (1, 2), (1, 3)]);
        let query = |route: i64| library_model::LibraryBrowseQuery {
            location: library_model::BrowseLocation::Subject(route),
            search: String::new(),
            file_types: vec![],
            languages: vec![],
            chip_sort: library_model::BrowseChipSort::Alphabetical,
            book_sort: library_model::BrowseBookSort::Alphabetical,
            hide_finished: false,
            include_direct_child_books: false,
        };
        let writes = db.connection.total_changes();
        let root = db.browse_contents(&query(0), false).unwrap();
        assert_eq!(root.children.iter().map(|child| child.name.as_str()).collect::<Vec<_>>(), ["Transportation"]);
        let transportation = db.browse_contents(&query(1), false).unwrap();
        assert!(transportation.children.is_empty());
        assert_eq!(transportation.books.len(), 3);
        assert_eq!(db.connection.total_changes(), writes, "browsing must not perform compaction writes");
        insert(4, 5);
        assert_eq!(visible(), vec![1, 2, 3]);
        assert_eq!(owners(), vec![(1, 3), (2, 2), (3, 1), (3, 4), (3, 5)]);
        db.with_write_transaction(|tx| {
            tx.execute("DELETE FROM book_unified_concept WHERE book_row_id IN (4,5)", [])?;
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
        assert_eq!(visible(), vec![1]);
        assert_eq!(owners(), vec![(1, 1), (1, 2), (1, 3)]);
        db.with_write_transaction(|tx| {
            tx.execute("DELETE FROM book_unified_concept WHERE book_row_id=3", [])?;
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
        assert_eq!(visible(), Vec::<i64>::new());
        assert_eq!(owners(), vec![(0, 1), (0, 2)]);
        assert_eq!(db.connection.query_row("SELECT concept_id FROM book_unified_concept WHERE book_row_id=1", [], |row| row.get::<_, i64>(0)).unwrap(), 3);
    }

    #[test]
    fn empty_single_child_wrapper_absorbs_and_restores_grandchildren() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        // This fixture owns derived rows directly; do not enqueue canonical edits.

        db.connection
            .execute_batch(
                "DETACH curated; ATTACH ':memory:' AS curated;
            CREATE TABLE curated.taxonomy_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE TABLE curated.concept(concept_id INTEGER PRIMARY KEY,preferred_label TEXT NOT NULL);
            INSERT INTO curated.concept VALUES(1,'Science'),(2,'Physics'),(3,'Quantum'),(4,'Optics');
            CREATE TABLE curated.unified_concept_route(route_id INTEGER PRIMARY KEY,concept_id INTEGER NOT NULL,parent_route_id INTEGER NOT NULL);
            INSERT INTO curated.unified_concept_route VALUES(1,1,0),(2,2,1),(3,3,2),(4,4,2);
        ",
            )
            .unwrap();
        let insert = |range: std::ops::RangeInclusive<i64>| {
            db.with_write_transaction(|tx| {
                for id in range {
                    tx.execute("INSERT INTO book(content_hash,title,format) VALUES(printf('%064x',?1),printf('Book %d',?1),1)", [id])?;
                    tx.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES(?1,?2,?3,'',1)", rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), id, format!("book{id}.epub")])?;
                    let concept = if id <= 3 {
                        3
                    } else if id <= 6 {
                        4
                    } else {
                        1
                    };
                    tx.execute("INSERT INTO book_unified_concept(book_row_id,concept_id,mapper_version) VALUES(?1,?2,1)", [id, concept])?;
                }
                Ok(WriteOutcome::Commit(()))
            })
            .unwrap();
        };
        let visible = || db.load_subject_tree().unwrap().into_iter().filter(|(route, _)| *route != 0).map(|(route, node)| (route, node.parent)).collect::<Vec<_>>();
        insert(1..=6);
        assert_eq!(visible(), vec![(1, 0), (3, 1), (4, 1)]);
        let query = library_model::LibraryBrowseQuery {
            location: library_model::BrowseLocation::Subject(1),
            search: String::new(),
            file_types: vec![],
            languages: vec![],
            chip_sort: library_model::BrowseChipSort::Alphabetical,
            book_sort: library_model::BrowseBookSort::Alphabetical,
            hide_finished: false,
            include_direct_child_books: false,
        };
        let contents = db.browse_contents(&query, false).unwrap();
        assert_eq!(contents.children.iter().map(|child| child.name.as_str()).collect::<Vec<_>>(), ["Optics", "Quantum"]);
        assert!(contents.books.is_empty());
        insert(7..=7);
        assert_eq!(visible(), vec![(1, 0), (2, 1), (3, 2), (4, 2)]);
        db.with_write_transaction(|tx| {
            tx.execute("DELETE FROM book_unified_concept WHERE book_row_id=7", [])?;
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
        assert_eq!(visible(), vec![(1, 0), (3, 1), (4, 1)]);
        assert_eq!(db.connection.query_row("SELECT concept_id FROM book_unified_concept WHERE book_row_id=1", [], |row| row.get::<_, i64>(0)).unwrap(), 3);
    }

    #[test]
    fn multiple_routes_filters_search_scope_and_taxonomy_edits_are_read_live() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.connection
            .execute_batch(
                "
            DETACH curated; ATTACH ':memory:' AS curated;
            CREATE TABLE curated.concept(concept_id INTEGER PRIMARY KEY,preferred_label TEXT NOT NULL);
            INSERT INTO curated.concept VALUES(1,'Science'),(2,'Physics'),(3,'Other'),(4,'Quantum');
            CREATE TABLE curated.unified_concept_route(route_id INTEGER PRIMARY KEY,concept_id INTEGER NOT NULL,parent_route_id INTEGER NOT NULL);
            INSERT INTO curated.unified_concept_route VALUES(1,1,0),(2,2,1),(3,3,0),(4,4,2),(5,4,3);",
            )
            .unwrap();
        for id in 1..=3 {
            db.connection.execute("INSERT INTO book(row_id,content_hash,title,search_title,format) VALUES(?1,printf('%064x',?1),printf('Book %d',?1),printf('book %d',?1),1)", [id]).unwrap();
            db.connection
                .execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES(?1,?2,?3,'',?4)", rusqlite::params![sync_common::ROOT_DIR_ID.to_string(), id, format!("book{id}.epub"), i64::from(id == 1)])
                .unwrap();
            db.connection.execute("INSERT INTO book_unified_concept VALUES(?1,4,1)", [id]).unwrap();
            db.connection.execute("INSERT INTO book_language VALUES(?1,?2)", rusqlite::params![id, if id == 1 { "en" } else { "sv" }]).unwrap();
        }
        db.connection.execute("INSERT INTO book_unified_concept VALUES(1,3,1)", []).unwrap();
        let mut query = library_model::LibraryBrowseQuery {
            location: library_model::BrowseLocation::Subject(0),
            search: String::new(),
            file_types: vec![],
            languages: vec![],
            chip_sort: Default::default(),
            book_sort: Default::default(),
            hide_finished: false,
            include_direct_child_books: false,
        };
        db.connection.pragma_update(None, "query_only", true).unwrap();
        let root = db.browse(&query, false).unwrap();
        assert_eq!(root.contents.children.iter().map(|row| (row.name.as_str(), row.book_count)).collect::<Vec<_>>(), [("Other", 3), ("Science", 3)]);
        assert_eq!(root.sections.len(), 2);
        query.search = "book".into();
        assert_eq!(db.browse_contents(&query, false).unwrap().books.len(), 3, "multiple routes must not duplicate cards");
        query.location = library_model::BrowseLocation::Subject(1);
        query.search = "Other".into();
        let searched = db.browse_contents(&query, false).unwrap();
        assert!(searched.books.is_empty() && searched.children.is_empty(), "search stays inside the selected subject");
        query.location = library_model::BrowseLocation::Subject(0);
        query.search.clear();
        query.languages = vec!["en".into()];
        let filtered = db.browse_contents(&query, false).unwrap();
        assert_eq!(filtered.children.iter().map(|row| (row.name.as_str(), row.book_count)).collect::<Vec<_>>(), [("Other", 1), ("Science", 1)], "filters must not change compaction");
        query.languages.clear();
        let downloaded = db.browse_contents(&query, true).unwrap();
        assert!(downloaded.children.iter().all(|row| row.book_count == 1 && row.downloaded_book_count == 1));
        db.connection.pragma_update(None, "query_only", false).unwrap();
        db.connection.execute("UPDATE curated.concept SET preferred_label='Renamed science' WHERE concept_id=1", []).unwrap();
        assert!(db.browse_contents(&query, false).unwrap().children.iter().any(|row| row.name == "Renamed science"));
        db.connection.execute("DELETE FROM book_dir WHERE book_row_id=3", []).unwrap();
        let collapsed = db.browse_contents(&query, false).unwrap();
        assert!(collapsed.children.is_empty());
        assert_eq!(collapsed.books.len(), 2);
        assert_eq!(db.connection.query_row("SELECT count(*) FROM book_unified_concept", [], |row| row.get::<_, i64>(0)).unwrap(), 4, "compaction never rewrites assignments");
    }
}
