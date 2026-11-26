//! Subject browsing uses one immutable packed snapshot per prepared result.

use library_model::{BrowseLocation, BrowsePathSegment};
use rusqlite::Connection;

use crate::{Database, DatabaseError};

mod snapshot;

/// The database whose book row IDs occur in the subject assignments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Books {
    Library,
    Catalog,
}

impl Books {
    pub(super) fn location(self, route: i64) -> BrowseLocation {
        match self {
            Self::Library => BrowseLocation::Subject(route),
            Self::Catalog => BrowseLocation::Catalog(route),
        }
    }
}

impl Database {
    /// Subject paths follow direct assignments after unfiltered compaction.
    pub(crate) fn book_subjects(&self, source: Books, book: i64) -> Result<Vec<Vec<BrowsePathSegment>>, DatabaseError> {
        let read = |catalog: Option<&Connection>| {
            let snapshot = self.subject_snapshot(source, catalog, None)?;
            let mut paths =
                snapshot.hierarchy.nodes.iter().enumerate().filter(|(index, _)| *index != 0 && snapshot.direct(*index).binary_search(&book).is_ok()).map(|(index, node)| (node.row.id, snapshot.hierarchy.path(index))).collect::<Vec<_>>();
            paths.sort_by_key(|(location, _)| *location);
            Ok(paths.into_iter().map(|(_, path)| path).collect())
        };
        match source {
            Books::Library => read(None),
            Books::Catalog => crate::catalog::read(|catalog| read(Some(catalog))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transactions::WriteOutcome;

    #[test]
    fn subject_cards_navigate_by_id_and_breadcrumbs_follow_compacted_parents() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.connection.fixtures_detach_curated_concept().unwrap();
        for id in 1..=6 {
            db.connection.fixtures_insert_book_6(&id).unwrap();
            db.connection.fixtures_insert_book_dir(&(sync_common::ROOT_DIR_ID.to_string()), &id, &(format!("book{id}.epub"))).unwrap();
            db.connection.fixtures_insert_book_unified_concept_2(&id, &(if id <= 3 { 3 } else { 4 })).unwrap();
        }
        let mut query = library_model::LibraryBrowseQuery {
            location: BrowseLocation::Subject(library_model::ROOT_SUBJECT_ROUTE_ID),
            search: String::new(),
            file_types: vec![],
            languages: vec![],
            chip_sort: Default::default(),
            book_sort: Default::default(),
            hide_finished: false,
        };
        let root = db.browse(&query, false).unwrap();
        assert!(root.path.is_empty());
        assert_eq!(root.contents.children[0].id, BrowseLocation::Subject(1));
        let science = &root.sections[0];
        assert_eq!(science.parent.id, BrowseLocation::Subject(1));
        assert_eq!(science.children.iter().map(|child| (child.id, child.name.as_str())).collect::<Vec<_>>(), [(BrowseLocation::Subject(4), "Optics"), (BrowseLocation::Subject(3), "Quantum")]);
        query.location = science.children[1].id;
        let quantum = db.browse(&query, false).unwrap();
        assert_eq!(quantum.contents.books.len(), 3);
        assert_eq!(quantum.path.iter().map(|segment| (segment.id, segment.name.as_str())).collect::<Vec<_>>(), [(BrowseLocation::Subject(1), "Science"), (BrowseLocation::Subject(3), "Quantum")]);
        db.connection.fixtures_update_curated_concept_preferred_label_fields().unwrap();
        let renamed = db.browse(&query, false).unwrap();
        assert_eq!(renamed.contents.books.len(), 3);
        assert_eq!(renamed.path.iter().map(|segment| (segment.id, segment.name.as_str())).collect::<Vec<_>>(), [(BrowseLocation::Subject(1), "1 / science"), (BrowseLocation::Subject(3), "Quantum / fields")]);
        query.location = BrowseLocation::Subject(library_model::ROOT_SUBJECT_ROUTE_ID);
        query.search = "fields".into();
        assert_eq!(db.browse(&query, false).unwrap().contents.children[0].id, BrowseLocation::Subject(3));
        assert!(db.connection.is_autocommit());
    }
    #[test]
    fn read_time_compaction_restores_and_recascades_after_assignment_changes() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        // This fixture owns derived rows directly; do not enqueue canonical edits.

        db.connection.fixtures_detach_curated_taxonomy_meta().unwrap();
        let insert = |first: i64, last: i64| {
            db.with_write_transaction(|tx| {
                for id in first..=last {
                    tx.fixtures_insert_book_8(&id)?;
                    tx.fixtures_insert_book_dir(&(sync_common::ROOT_DIR_ID.to_string()), &id, &(format!("book{id}.epub")))?;
                    let concept = match id {
                        1 | 4 | 5 => 3,
                        2 => 4,
                        3 => 5,
                        _ => unreachable!(),
                    };
                    tx.fixtures_insert_book_unified_concept_3(&id, &concept)?;
                }
                Ok(WriteOutcome::Commit(()))
            })
            .unwrap();
        };
        let owners = || {
            let snapshot = db.subject_snapshot(Books::Library, None, None).unwrap();
            let mut owners = snapshot.hierarchy.nodes.iter().enumerate().flat_map(|(index, node)| snapshot.direct(index).iter().map(move |&book| (node.row.id, book))).collect::<Vec<_>>();
            owners.sort();
            owners
                .into_iter()
                .map(|(location, book)| match location {
                    BrowseLocation::Subject(route) => (route, book),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>()
        };
        let visible = || {
            let snapshot = db.subject_snapshot(Books::Library, None, None).unwrap();
            let mut routes = snapshot
                .hierarchy
                .nodes
                .iter()
                .skip(1)
                .map(|node| match node.row.id {
                    BrowseLocation::Subject(route) => route,
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>();
            routes.sort();
            routes
        };
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
            tx.fixtures_delete_book_unified_concept()?;
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
        assert_eq!(visible(), vec![1]);
        assert_eq!(owners(), vec![(1, 1), (1, 2), (1, 3)]);
        db.with_write_transaction(|tx| {
            tx.fixtures_delete_book_unified_concept_2()?;
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
        assert_eq!(visible(), Vec::<i64>::new());
        assert_eq!(owners(), vec![(0, 1), (0, 2)]);
        assert_eq!(db.connection.fixtures_read_book_unified_concept_concept_id(|row| row.get::<_, i64>(0)).unwrap(), 3);
    }

    #[test]
    fn empty_single_child_wrapper_absorbs_and_restores_grandchildren() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        // This fixture owns derived rows directly; do not enqueue canonical edits.

        db.connection.fixtures_detach_curated_taxonomy_meta_2().unwrap();
        let insert = |range: std::ops::RangeInclusive<i64>| {
            db.with_write_transaction(|tx| {
                for id in range {
                    tx.fixtures_insert_book_8(&id)?;
                    tx.fixtures_insert_book_dir(&(sync_common::ROOT_DIR_ID.to_string()), &id, &(format!("book{id}.epub")))?;
                    let concept = if id <= 3 {
                        3
                    } else if id <= 6 {
                        4
                    } else {
                        1
                    };
                    tx.fixtures_insert_book_unified_concept_3(&id, &concept)?;
                }
                Ok(WriteOutcome::Commit(()))
            })
            .unwrap();
        };
        let visible = || {
            let snapshot = db.subject_snapshot(Books::Library, None, None).unwrap();
            let mut routes = snapshot
                .hierarchy
                .nodes
                .iter()
                .skip(1)
                .map(|node| match (node.row.id, snapshot.hierarchy.nodes[node.parent.unwrap()].row.id) {
                    (BrowseLocation::Subject(route), BrowseLocation::Subject(parent)) => (route, parent),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>();
            routes.sort();
            routes
        };
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
        };
        let contents = db.browse_contents(&query, false).unwrap();
        assert_eq!(contents.children.iter().map(|child| child.name.as_str()).collect::<Vec<_>>(), ["Optics", "Quantum"]);
        assert!(contents.books.is_empty());
        insert(7..=7);
        assert_eq!(visible(), vec![(1, 0), (2, 1), (3, 2), (4, 2)]);
        db.with_write_transaction(|tx| {
            tx.fixtures_delete_book_unified_concept_3()?;
            Ok(WriteOutcome::Commit(()))
        })
        .unwrap();
        assert_eq!(visible(), vec![(1, 0), (3, 1), (4, 1)]);
        assert_eq!(db.connection.fixtures_read_book_unified_concept_concept_id(|row| row.get::<_, i64>(0)).unwrap(), 3);
    }

    #[test]
    fn multiple_routes_filters_search_scope_and_taxonomy_edits_are_read_live() {
        let db = Database::open(":memory:").unwrap();
        db.initialize_library().unwrap();
        db.connection.fixtures_detach_curated_concept_2().unwrap();
        for id in 1..=3 {
            db.connection.fixtures_insert_book_6(&id).unwrap();
            db.connection.fixtures_insert_book_dir_2(&(sync_common::ROOT_DIR_ID.to_string()), &id, &(format!("book{id}.epub")), &(i64::from(id == 1))).unwrap();
            db.connection.fixtures_insert_book_unified_concept_4(&id).unwrap();
            db.connection.fixtures_insert_book_language_2(&id, &(if id == 1 { "en" } else { "sv" })).unwrap();
        }
        db.connection.fixtures_insert_book_unified_concept_5().unwrap();
        let mut query = library_model::LibraryBrowseQuery {
            location: library_model::BrowseLocation::Subject(0),
            search: String::new(),
            file_types: vec![],
            languages: vec![],
            chip_sort: Default::default(),
            book_sort: Default::default(),
            hide_finished: false,
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
        db.connection.fixtures_update_curated_concept_preferred_label_science().unwrap();
        assert!(db.browse_contents(&query, false).unwrap().children.iter().any(|row| row.name == "Renamed science"));
        db.connection.fixtures_delete_book_dir().unwrap();
        let collapsed = db.browse_contents(&query, false).unwrap();
        assert!(collapsed.children.is_empty());
        assert_eq!(collapsed.books.len(), 2);
        assert_eq!(db.connection.fixtures_read_book_unified_concept_count(|row| row.get::<_, i64>(0)).unwrap(), 4, "compaction never rewrites assignments");
    }
}

#[cfg(test)]
use crate::sql_fixtures::FixturesSql;
