use crate::Database;
use crate::browse::*;
use library_model::{BrowseBookSort, BrowseChipSort, LibraryFileTypeFilter};

use crate::test_support::TestDatabase;

fn open_test_database() -> TestDatabase {
    TestDatabase::new("browse")
}

#[test]
fn languages_are_stored_by_base_language() {
    for (tag, expected) in [(" EN_us ", "en"), ("en-GB", "en"), ("sv", "sv"), ("", "")] {
        assert_eq!(options::base_language(tag), expected, "{tag:?}");
    }
}

#[test]
fn directory_hierarchy_browses() {
    let holder = open_test_database();
    let database = &holder.database;
    let parent = database.create_directory_with_id(&uuid::Uuid::new_v4(), &sync_common::ROOT_DIR_ID, &"Parent".to_owned()).expect("seed parent");
    let child = database.create_directory_with_id(&uuid::Uuid::new_v4(), &parent.id, &"Child".to_owned()).expect("seed child");
    assert_eq!(database.browse_book_count().expect("book count"), 0);

    let contents = database.browse_folder_contents(&page_query(sync_common::ROOT_DIR_ID.to_string()), false).expect("folder contents");
    assert!(contents.books.is_empty());
    assert!(contents.children.iter().any(|row| row.name == "Parent" && row.book_count == 0));

    let path = database.browse_directory_path(&child.id).expect("directory path");
    assert_eq!(path.last().map(|segment| segment.name.as_str()), Some("Child"));

    let page = database.browse_folder(&page_query(sync_common::ROOT_DIR_ID.to_string()), false).expect("folder page");
    assert!(page.language_counts.is_empty() && page.format_counts.is_empty());

    let home = database.browse_home(10, false).expect("home view");
    assert!(home.most_progress.is_empty() && home.recently_read.is_empty() && home.recently_added.is_empty());
    assert!(database.browse_authors(library_model::AuthorSort::Name, false).expect("authors").is_empty());
}

fn inspected(title: &str, author: &str) -> book_metadata::InspectedBook {
    book_metadata::InspectedBook {
        pdf: None,
        metadata: book_model::BookRecord {
            title: title.to_owned(),
            subtitle: None,
            contributors: vec![book_model::Contributor::new(author, book_model::MarcRelatorCode(*b"aut")).unwrap()],
            description: "A synopsis.".to_owned(),
            book: book_model::BookMetadata::default(),
        },
        audiobook: None,
        toc: Vec::new(),
    }
}

fn commit_book(database: &Database, parent: &sync_common::DirId, digit: &str, title: &str, author: &str, file_name: &str) -> sync_common::ContentHash {
    use crate::import::{ImportCommit, PublishedImport};
    let hash = sync_common::ContentHash::new(&digit.repeat(64));
    database
        .commit_import(ImportCommit {
            parent_id: parent.clone(),
            file_name: file_name.to_owned(),
            format: book_model::BookFormat::Epub,
            content_hash: hash,
            checksum: sync_common::ContentHash::new(&"a".repeat(64)),
            size_bytes: 1024,
            inspection_version: 1,
            inspection: Some(inspected(title, author)),
            published: PublishedImport { name: file_name.to_owned(), relative_path: file_name.to_owned(), published: true, fingerprint: None },
            request_thumbnail: false,
            restore_paths: Vec::new(),
        })
        .expect("commit import");
    hash
}

#[test]
fn folder_search_finds_title_and_author_matches() {
    let holder = open_test_database();
    let database = &holder.database;
    let parent = database.create_directory(&sync_common::ROOT_DIR_ID, &"Fiction".to_owned()).expect("create folder");
    let rust = commit_book(database, &parent.id, "9", "The Rust Programming Language", "Steve Klabnik", "rust.epub");
    let cooking = commit_book(database, &parent.id, "8", "Goran's Cookbook", "Ada Lovelace", "cookbook.epub");
    let search = |text: &str| {
        let mut query = page_query(sync_common::ROOT_DIR_ID.to_string());
        query.search = text.into();
        database.browse_folder_contents(&query, false).expect("search").books.iter().map(|card| card.content_hash).collect::<Vec<_>>()
    };
    assert_eq!(search("rust"), [rust]);
    assert_eq!(search("lovelace"), [cooking]);
    assert!(search("missing").is_empty());
}

fn page_query(location: String) -> library_model::LibraryBrowseQuery {
    library_model::LibraryBrowseQuery {
        location,
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
fn complete_pages_keep_cards_facets_and_paths_on_one_snapshot() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };
    for subject in [false, true] {
        let holder = open_test_database();
        let db = &holder.database;
        let directory = db.create_directory(&sync_common::ROOT_DIR_ID, &"Original folder".into()).unwrap().id;
        commit_book(db, &directory, "1", "Original book", "Writer", "book.epub");
        db.connection.execute("INSERT INTO book_language SELECT row_id,'en' FROM book", []).unwrap();
        db.connection.execute("INSERT INTO book_unified_concept SELECT row_id,(SELECT concept_id FROM curated.unified_concept_route WHERE concept_id>0 LIMIT 1),1 FROM book", []).unwrap();
        let query = page_query(if subject { "0".into() } else { directory.to_string() });
        let browse = || if subject { db.browse_subject(&query, false) } else { db.browse_folder(&query, false) };
        // Build membership first so the hook only intercepts a clean read.
        browse().unwrap();
        let fired = Arc::new(AtomicBool::new(false));
        let observed = fired.clone();
        let mutation = Arc::new(Mutex::new(None));
        let result = mutation.clone();
        let path = holder.path.clone();
        db.connection.authorizer(Some(move |context: AuthContext<'_>| {
            if matches!(context.action, AuthAction::Read { table_name: "book", column_name: "title" }) && !observed.swap(true, Ordering::SeqCst) {
                let update = (|| -> Result<(), crate::DatabaseError> {
                    let writer = Database::open(&path)?;
                    writer.connection.execute_batch("UPDATE sync_metadata SET change_origin='remote'; UPDATE book SET title='Changed book',format=2; UPDATE book_language SET language='sv';")?;
                    writer.connection.execute("UPDATE dir SET name='Changed folder' WHERE id=?1", [directory.to_string()])?;
                    Ok(())
                })();
                *result.lock().unwrap() = Some(update.map_err(|error| error.to_string()));
            }
            Authorization::Allow
        }))
        .unwrap();
        let page = browse().unwrap();
        db.connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>).unwrap();
        assert!(fired.load(Ordering::SeqCst), "concurrent mutation must be exercised");
        assert_eq!(mutation.lock().unwrap().as_ref(), Some(&Ok(())), "WAL writer must complete during browsing");
        assert_eq!(page.contents.books[0].title, "Original book");
        assert_eq!(page.format_counts[0].format, LibraryFileTypeFilter::Book);
        assert_eq!(page.language_counts[0].language, "en");
        if !subject {
            assert_eq!(page.path.last().unwrap().name, "Original folder");
        }
        let updated = browse().unwrap();
        assert_eq!(updated.contents.books[0].title, "Changed book");
        assert_eq!(updated.format_counts[0].format, LibraryFileTypeFilter::Pdf);
        assert_eq!(updated.language_counts[0].language, "sv");
        if !subject {
            assert_eq!(updated.path.last().unwrap().name, "Changed folder");
        }
    }
}

#[test]
fn card_queries_return_the_same_named_fields() {
    let holder = open_test_database();
    let db = &holder.database;
    let directory = sync_common::ROOT_DIR_ID;
    let hash = commit_book(db, &directory, "2", "First book", "Shared author", "first.epub");
    commit_book(db, &directory, "3", "Second book", "Shared author", "second.epub");
    db.connection.execute_batch("UPDATE book SET added_at=row_id*1000,read_progress=25;").unwrap();
    db.connection.execute("INSERT INTO book_unified_concept SELECT row_id,(SELECT concept_id FROM curated.unified_concept_route WHERE concept_id>0 LIMIT 1),1 FROM book", []).unwrap();
    db.connection.execute("UPDATE sync_metadata SET change_origin='local'", []).unwrap();
    db.update_reading_position(&hash, "epubcfi(/6/2!/4/2/1:0)", Some(25.), None).unwrap();
    db.connection.execute("INSERT OR IGNORE INTO book_unified_concept SELECT row_id,(SELECT concept_id FROM curated.unified_concept_route WHERE concept_id>0 LIMIT 1),1 FROM book", []).unwrap();
    let expected = db.browse_book_detail(hash).unwrap().book;
    assert_eq!(expected.added_at, 1000);
    let home = db.browse_home(10, false).unwrap();
    let authors = db.browse_authors(library_model::AuthorSort::Name, false).unwrap();
    let subject = db.browse_subject_contents(&page_query("0".into()), false).unwrap();
    for cards in [&home.most_progress, &home.recently_read, &home.recently_added, &authors[0].books, &subject.books] {
        assert_eq!(cards.iter().find(|card| card.content_hash == hash).unwrap(), &expected);
    }
    let mut query = page_query(directory.to_string());
    query.file_types = vec![LibraryFileTypeFilter::Book];
    let folder = db.browse_folder_contents(&query, false).unwrap();
    let card = folder.books.iter().find(|card| card.content_hash == hash).unwrap();
    assert_eq!(card.added_at, expected.added_at);
    assert_eq!(card.source_directory, Some(directory));
}

#[test]
fn nullable_card_fields_do_not_hide_invalid_data() {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    let sql = "SELECT ?1 AS title, ?2 AS audiobook_duration_ms,
        '0000000000000000000000000000000000000000000000000000000000000000' AS content_hash,
        NULL AS author,NULL AS subtitle,NULL AS progress,
        0 AS downloaded,0 AS download_requested,1 AS format,
        NULL AS added_at";
    let null = rusqlite::types::Value::Null;
    let card = connection.query_row(sql, [&null, &null], read_book_card_row).unwrap();
    assert!(card.title.starts_with("Untitled"));
    assert_eq!(card.added_at, 0);
    for values in [[rusqlite::types::Value::Integer(7), null.clone()], [null.clone(), rusqlite::types::Value::Integer(-1)]] {
        assert!(connection.query_row(sql, values, read_book_card_row).is_err());
    }
    assert!(connection.query_row(&sql.replace("AS audiobook_duration_ms", "AS missing_column"), [&null, &null], read_book_card_row).is_err());
}

#[test]
fn scoped_search_filters_card_formats_and_membership() {
    let holder = open_test_database();
    let db = &holder.database;
    let shelf = db.create_directory(&sync_common::ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    let nested = db.create_directory(&shelf, &"Nested".into()).unwrap().id;
    let pdf = commit_book(db, &nested, "4", "Shared title", "Writer", "first.pdf");
    let epub = commit_book(db, &shelf, "5", "Another title", "Shared author", "second.epub");
    commit_book(db, &sync_common::ROOT_DIR_ID, "6", "Shared elsewhere", "Writer", "outside.epub");
    db.connection.execute("UPDATE sync_metadata SET change_origin='remote'",[]).unwrap();
    db.connection.execute("UPDATE book SET format=2 WHERE content_hash=?1", [pdf.as_str()]).unwrap();
    db.connection.execute("INSERT INTO book_unified_concept SELECT row_id,(SELECT concept_id FROM curated.unified_concept_route WHERE concept_id>0 LIMIT 1),1 FROM book WHERE content_hash=?1", [pdf.as_str()]).unwrap();
    let search = |location: String, file_type: LibraryFileTypeFilter| {
        let mut query = page_query(location);
        query.search = "shared".into();
        query.file_types = vec![file_type];
        let contents = if query.location == "0" { db.browse_subject_contents(&query, false) } else { db.browse_folder_contents(&query, false) };
        contents.unwrap().books.iter().map(|card| card.content_hash).collect::<Vec<_>>()
    };
    assert_eq!(search(shelf.to_string(), LibraryFileTypeFilter::Pdf), [pdf]);
    assert_eq!(search(shelf.to_string(), LibraryFileTypeFilter::Book), [epub]);
    assert_eq!(search("0".into(), LibraryFileTypeFilter::All), [pdf]);
    assert!(search("0".into(), LibraryFileTypeFilter::Book).is_empty());
}

#[test]
fn blank_language_selections_and_home_limits() {
    let holder = open_test_database();
    let db = &holder.database;
    let hash = commit_book(db, &sync_common::ROOT_DIR_ID, "7", "Book", "Author", "book.epub");
    db.update_reading_position(&hash, "epubcfi(/6/2!/4/2/1:0)", Some(25.), None).unwrap();
    db.connection.execute("INSERT INTO book_language SELECT row_id,'en' FROM book", []).unwrap();
    db.connection.execute("INSERT INTO book_unified_concept SELECT row_id,(SELECT concept_id FROM curated.unified_concept_route WHERE concept_id>0 LIMIT 1),1 FROM book", []).unwrap();
    for location in [sync_common::ROOT_DIR_ID.to_string(), "0".into()] {
        let mut query = page_query(location.clone());
        let browse = |query: &library_model::LibraryBrowseQuery| if location == "0" { db.browse_subject(query, false) } else { db.browse_folder(query, false) }.unwrap();
        let expected = browse(&query);
        for languages in [vec!["".into(), "  ".into()], vec!["".into(), " EN_us ".into(), "en-GB".into()]] {
            query.languages = languages;
            let actual = browse(&query);
            assert_eq!(actual.contents, expected.contents);
            assert_eq!(actual.language_counts, expected.language_counts);
            assert_eq!(actual.format_counts, expected.format_counts);
        }
    }
    for limit in [-1, i32::MIN] {
        assert!(db.browse_home(limit, false).unwrap_err().to_string().contains("limit"));
    }
    for limit in [0, 1] {
        let view = db.browse_home(limit, false).unwrap();
        assert_eq!(view.most_progress.len(), limit as usize);
        assert_eq!(view.recently_read.len(), limit as usize);
        assert_eq!(view.recently_added.len(), limit as usize);
    }
}

#[test]
fn home_authors_and_detail_hold_a_snapshot_between_component_queries() {
    use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
    use std::sync::{Arc, Mutex};
    // Each column is first read by a view's second or later query: SQLite
    // starts the read snapshot at the first read, not at BEGIN.
    for (view, table, column) in [("home", "book", "read_at"), ("authors", "book_contributor", "name"), ("detail", "book_dir", "dir_id")] {
        let holder = open_test_database();
        let db = &holder.database;
        let directory = db.create_directory(&sync_common::ROOT_DIR_ID, &"Original folder".into()).unwrap().id;
        let hash = commit_book(db, &directory, "8", "Original book", "Author", "book.epub");
        db.update_reading_position(&hash, "epubcfi(/6/2!/4/2/1:0)", Some(25.), None).unwrap();
    db.connection.execute("INSERT OR IGNORE INTO book_unified_concept SELECT row_id,(SELECT concept_id FROM curated.unified_concept_route WHERE concept_id>0 LIMIT 1),1 FROM book", []).unwrap();
        let outcome = Arc::new(Mutex::new(None));
        let observed = outcome.clone();
        let path = holder.path.clone();
        db.connection.authorizer(Some(move |context: AuthContext<'_>| {
            if let AuthAction::Read { table_name, column_name } = context.action {
                let mut result = observed.lock().unwrap();
                if table_name == table && column_name == column && result.is_none() {
                    *result = Some(
                        (|| -> Result<(), crate::DatabaseError> {
                            let writer = Database::open(&path)?;
                            writer.connection.execute_batch("UPDATE sync_metadata SET change_origin='remote'; UPDATE book SET title='Changed'; UPDATE book_dir SET deleted_at=1;")?;
                            Ok(())
                        })()
                        .map_err(|error| error.to_string()),
                    );
                }
            }
            Authorization::Allow
        }))
        .unwrap();
        match view {
            "home" => {
                let home = db.browse_home(10, false).unwrap();
                for cards in [home.most_progress, home.recently_read, home.recently_added] {
                    assert_eq!(cards.len(), 1);
                    assert_eq!(cards[0].title, "Original book");
                }
            }
            "authors" => {
                let authors = db.browse_authors(library_model::AuthorSort::Name, false).unwrap();
                assert_eq!(authors.len(), 1);
                assert_eq!(authors[0].book_count, 1);
                assert_eq!(authors[0].books[0].title, "Original book");
            }
            _ => {
                let detail = db.browse_book_detail(hash).unwrap();
                assert_eq!(detail.book.title, "Original book");
                assert_eq!(detail.source_directory_id, Some(directory.to_string()));
                assert_eq!(detail.path.last().unwrap().name, "Original folder");
            }
        }
        db.connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>).unwrap();
        assert_eq!(outcome.lock().unwrap().as_ref(), Some(&Ok(())), "{view}: writer ran between component queries");
        assert_eq!(db.browse_book_count().unwrap(), 0, "{view}: snapshot released");
        assert!(db.connection.is_autocommit());
    }
}

#[test]
fn failed_view_read_releases_its_snapshot() {
    let holder = open_test_database();
    let db = &holder.database;
    let missing = sync_common::ContentHash::new(&"f".repeat(64));
    assert!(db.browse_book_detail(missing).is_err());
    assert!(db.connection.is_autocommit());
    commit_book(db, &sync_common::ROOT_DIR_ID, "b", "Recovered", "Writer", "book.epub");
    assert_eq!(db.browse_home(1, false).unwrap().recently_added.len(), 1);
}

fn assert_untitled_search(db: &Database, hash: sync_common::ContentHash, search: &str, expected_count: usize) {
    for location in [sync_common::ROOT_DIR_ID.to_string(), "0".into()] {
        let mut query = page_query(location.clone());
        query.search = search.into();
        let page = if location == "0" { db.browse_subject(&query, false) } else { db.browse_folder(&query, false) }.unwrap();
        assert_eq!(page.contents.books.len(), expected_count, "{location}: {search}");
        assert_eq!(page.format_counts.iter().map(|count| count.book_count as usize).sum::<usize>(), 1);
        assert_eq!(page.language_counts.iter().map(|count| count.book_count as usize).sum::<usize>(), 1);
        if expected_count != 0 {
            assert_eq!(page.contents.books[0].content_hash, hash);
            assert_eq!(page.format_counts[0].format, LibraryFileTypeFilter::Book);
            assert_eq!(page.language_counts[0].language, "en");
        }
    }
}

#[test]
fn untitled_books_share_search_matches_and_facets_across_views() {
    let holder = open_test_database();
    let db = &holder.database;
    // This browse fixture writes book rows directly, so it stores the browse
    // text the Rust write paths would, including NULL versus empty title.
    db.connection.execute("UPDATE sync_metadata SET change_origin='remote'",[]).unwrap();
    let hash = sync_common::ContentHash::new(&"c".repeat(64));
    let set_title = |title: Option<&str>, subtitle: Option<&str>| {
        db.connection.execute("UPDATE book SET title=?1,subtitle=?2,search_title=?3", rusqlite::params![title, subtitle, text::search_title(hash.as_str(), title, subtitle)]).unwrap();
    };
    db.connection.execute("INSERT INTO book(content_hash,format) VALUES(?1,1)", [hash.as_str()]).unwrap();
    set_title(None, Some("Écology"));
    db.connection.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) SELECT ?1,row_id,'untitled.epub','',1 FROM book", [sync_common::ROOT_DIR_ID.to_string()]).unwrap();
    db.sync_publishable_mutations().unwrap();
    db.connection.execute("UPDATE sync_metadata SET change_origin='remote'",[]).unwrap();
    db.connection.execute("INSERT INTO book_language SELECT row_id,'en' FROM book", []).unwrap();
    db.connection.execute("INSERT INTO book_unified_concept SELECT row_id,(SELECT concept_id FROM curated.unified_concept_route WHERE concept_id>0 LIMIT 1),1 FROM book", []).unwrap();
    for search in ["untitled", hash.as_str(), "ecology"] {
        assert_untitled_search(db, hash, search, 1);
    }
    let display = format!("Untitled ({hash}): Écology");
    assert_untitled_search(db, hash, &display, 1);
    assert_eq!(db.browse_book_detail(hash).unwrap().book.title, format!("Untitled ({hash})"));

    for title in ["Named book", ""] {
        set_title(Some(title), Some("Écology"));
        assert_untitled_search(db, hash, "untitled", 0);
        assert_untitled_search(db, hash, hash.as_str(), 0);
        assert_untitled_search(db, hash, "ecology", 1);
    }
    set_title(None, None);
    assert_untitled_search(db, hash, "untitled", 1);
    assert_untitled_search(db, hash, "ecology", 0);
}
