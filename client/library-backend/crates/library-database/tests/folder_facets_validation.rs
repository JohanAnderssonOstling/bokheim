//! Direct folder placement queries, read-time counts, and format facets.
use library_database::{Database, rusqlite::params};
use library_model::LibraryFileTypeFilter;
const ROOT: &str = "00000000-0000-0000-0000-000000000000";
const TARGET: &str = "00000000-0000-0000-0000-000000000001";
const CHILD: &str = "00000000-0000-0000-0000-000000000002";
const BRANCH: &str = "00000000-0000-0000-0000-000000000003";
const OTHER: &str = "00000000-0000-0000-0000-000000000004";
fn fixture(books: usize) -> Database {
    let db = Database::open(":memory:").unwrap();
    db.initialize_library().unwrap();

    let tx = db.connection.unchecked_transaction().unwrap();
    for (id, parent, name) in [(TARGET, ROOT, "Target"), (CHILD, TARGET, "Nested"), (BRANCH, TARGET, "Branch"), (OTHER, ROOT, "Targetish")] {
        tx.execute("INSERT INTO dir(id,parent_id,name) VALUES(?1,?2,?3)", [id, parent, name]).unwrap();
    }
    for i in 1..=books as i64 {
        tx.execute("INSERT INTO book(content_hash,title,format,search_title) VALUES(?1,?2,?3,normalize_search_text(?2))", params![format!("{i:064x}"), format!("Book {i}"), [1, 2, 8][i as usize % 3]]).unwrap();
        tx.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES(?1,?2,?3,'',?4)", params![[TARGET, CHILD, BRANCH, OTHER][i as usize % 4], i, format!("{i}.epub"), i % 2]).unwrap();
        if i % 11 == 0 {
            tx.execute("INSERT OR IGNORE INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES(?1,?2,?3,'',1)", params![OTHER, i, format!("extra{i}.epub")]).unwrap();
        }
        tx.execute("INSERT INTO book_language VALUES(?1,?2)", params![i, if i % 3 == 0 { "sv" } else { "en" }]).unwrap();
        if i % 7 == 0 {
            tx.execute("INSERT OR IGNORE INTO book_language VALUES(?1,'en')", [i]).unwrap();
        }
    }
    tx.commit().unwrap();
    db
}
fn query(location: &str, search: &str) -> library_model::LibraryBrowseQuery {
    library_model::LibraryBrowseQuery {
        location: library_model::BrowseLocation::Folder(location.parse().unwrap()),
        search: search.into(),
        file_types: vec![],
        languages: vec![],
        chip_sort: Default::default(),
        book_sort: Default::default(),
        hide_finished: false,
        include_direct_child_books: false,
    }
}
#[test]
fn direct_descendants_and_directory_counts_follow_mutations() {
    let db = fixture(32);
    for mutation in [
        "SELECT 1",
        "UPDATE book_dir SET deleted_at=1 WHERE book_row_id%3=0",
        "UPDATE book SET deleted_at=1 WHERE row_id%7=0",
        "UPDATE dir SET parent_id='00000000-0000-0000-0000-000000000004' WHERE name='Nested'",
        "UPDATE dir SET deleted_at=1 WHERE name='Target'",
        "UPDATE dir SET deleted_at=NULL; UPDATE book SET format=8 WHERE row_id%2=0",
    ] {
        db.connection.execute_batch(mutation).unwrap();
        let ids = db.connection.prepare("SELECT id FROM dir").unwrap().query_map([], |r| r.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        for id in ids {
            for folder in db.browse_contents(&query(&id, ""), false).unwrap().children {
                let library_model::BrowseLocation::Folder(folder_id) = folder.id else { panic!("folder browse returned a subject") };
                let expected = db.connection.query_row("WITH RECURSIVE subtree(id) AS (SELECT ?1 UNION SELECT d.id FROM subtree s JOIN dir d ON d.parent_id=s.id WHERE d.id!=s.id AND d.deleted_at IS NULL) SELECT count(*),coalesce(sum(p.is_downloaded),0) FROM subtree s JOIN book_dir p ON p.dir_id=s.id AND p.deleted_at IS NULL JOIN book b ON b.row_id=p.book_row_id AND b.deleted_at IS NULL", [folder_id.to_string()], |r| Ok((r.get::<_, i64>(0)?,r.get::<_, i64>(1)?))).unwrap();
                assert_eq!((folder.book_count as i64, folder.downloaded_book_count as i64), expected, "{mutation}: {}", folder.id);
            }
        }
        // Both cards and counts reflect the committed writes immediately.
        let mut hashes = db.browse_contents(&query(TARGET, "book"), false).unwrap().books.iter().map(|b| b.content_hash.to_string()).collect::<Vec<_>>();
        hashes.sort();
        let mut stmt = db.connection.prepare("WITH RECURSIVE subtree(id) AS (SELECT id FROM dir WHERE id=?1 AND deleted_at IS NULL UNION SELECT d.id FROM dir d JOIN subtree s ON d.parent_id=s.id WHERE d.id!=d.parent_id AND d.deleted_at IS NULL) SELECT b.content_hash FROM book b JOIN book_dir p ON p.book_row_id=b.row_id JOIN subtree s ON s.id=p.dir_id WHERE b.deleted_at IS NULL AND p.deleted_at IS NULL ORDER BY b.content_hash").unwrap();
        let expected = stmt.query_map([TARGET], |r| r.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(hashes, expected, "{mutation}");
        let formats = db
            .browse(&query(ROOT, ""), false)
            .unwrap()
            .format_counts
            .iter()
            .map(|count| {
                let category = match count.format {
                    LibraryFileTypeFilter::Pdf => 2,
                    LibraryFileTypeFilter::Audiobook => 4,
                    _ => 1,
                };
                (category, i64::from(count.book_count))
            })
            .collect::<Vec<_>>();
        let expected = db.connection.prepare("SELECT CASE b.format WHEN 2 THEN 2 WHEN 8 THEN 4 ELSE 1 END AS category,COUNT(*) FROM book b WHERE b.deleted_at IS NULL AND EXISTS(SELECT 1 FROM book_dir p WHERE p.book_row_id=b.row_id AND p.deleted_at IS NULL) GROUP BY category ORDER BY category").unwrap()
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(formats, expected, "{mutation}");
    }
}
