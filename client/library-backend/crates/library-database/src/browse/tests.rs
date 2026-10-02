use crate::native::observations::forget_file;
use crate::browse::rows::read_book_card_row;
use crate::browse::*;
use crate::Database;
use library_model::{BrowseBookSort, BrowseChipSort, LibraryFileTypeFilter};
use rusqlite::OptionalExtension;

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
    assert_eq!(
        ({
            use crate::browse::{CountsSql, HomeSql};
            use crate::DatabaseError;
            use library_model::LibraryHomeView;
            let database = &database;
            'directory_hierarchy_browses: {
                let mut count = 0_i64;
                (match database.connection.count_all_books(|row| {
                    count = row.get(0)?;
                    Ok(())
                }) {
                    Ok(value) => value,
                    Err(error) => break 'directory_hierarchy_browses Err(error.into()),
                });
                Ok::<_, DatabaseError>(
                    (match usize::try_from(count).map_err(|_| DatabaseError::message("library book count must not be negative")) {
                        Ok(value) => value,
                        Err(error) => break 'directory_hierarchy_browses Err(error.into()),
                    }),
                )
            }
        })
        .expect("book count"),
        0
    );

    let contents = database.browse_contents(&page_query(library_model::BrowseLocation::Folder(sync_common::ROOT_DIR_ID)), false).expect("folder contents");
    assert!(contents.books.is_empty());
    assert!(contents.children.iter().any(|row| row.name == "Parent" && row.book_count == 0));

    let path = database.browse_directory_path(&child.id).expect("directory path");
    assert_eq!(path.last().map(|segment| segment.name.as_str()), Some("Child"));

    let page = database.browse(&page_query(library_model::BrowseLocation::Folder(sync_common::ROOT_DIR_ID)), false).expect("folder page");
    assert!(page.language_counts.is_empty() && page.format_counts.is_empty());

    let home = ({
        use crate::browse::{CountsSql, HomeSql};
        use crate::DatabaseError;
        use library_model::LibraryHomeView;
        let database = &database;
        let limit = 10;
        let downloaded_only = false;
        'home: {
            if limit < 0 {
                break 'home Err(DatabaseError::message("browse home limit must not be negative"));
            }
            let downloaded_only = i32::from(downloaded_only);
            database.with_browse_snapshot(|| {
                // The most recently read books: up to `limit` still in progress, then
                // up to `limit` finished. The two groups arrive in one statement and
                // stay in reading order within each, so a caller splits them by the
                // same threshold the query ranked them with.
                let mut read = Vec::new();
                database.connection.recently_read_books(limit, downloaded_only, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD), |row| {
                    read.push(row.get(0)?);
                    Ok(())
                })?;
                let mut added = Vec::new();
                database.connection.recently_added_books(limit, downloaded_only, |row| {
                    added.push(row.get(0)?);
                    Ok(())
                })?;
                Ok(LibraryHomeView { recently_read: database.load_cards(&read)?, recently_added: database.load_cards(&added)? })
            })
        }
    })
    .expect("home view");
    assert!(home.recently_read.is_empty() && home.recently_added.is_empty());
    assert!(({
        use crate::browse::rows::{invalid_integer_column, invalid_text_column};
        use crate::browse::AuthorsSql;
        use crate::{Database, DatabaseError};
        use book_model::{AgentId, AuthorId, AUTHOR_MARC_RELATOR_CODE};
        use library_model::{AuthorSummary, BookCardRow};
        use std::collections::HashMap;
        const AUTHOR_SHELF_BOOK_COUNT: i32 = 40;
        let database: &Database = &database;
        let sort = library_model::AuthorSort::Name;
        let downloaded_only = false;

        database.with_browse_snapshot(|| {
            let mut shelves = {
                let per_author: i32 = AUTHOR_SHELF_BOOK_COUNT;
                let (mut authors, mut ids) = (Vec::new(), Vec::new());
                database.connection.author_shelf_books(i32::from(downloaded_only), per_author, AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                    let raw_id: String = row.get(0)?;
                    authors.push(AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?);
                    ids.push(row.get(1)?);
                    Ok(())
                })?;
                let mut shelves: HashMap<AuthorId, Vec<BookCardRow>> = HashMap::new();
                for (author, card) in authors.into_iter().zip(database.load_cards(&ids)?) {
                    shelves.entry(author).or_default().push(card);
                }
                shelves
            };
            let mut authors = {
                let mut authors = Vec::new();
                database.connection.get_authors(i32::from(downloaded_only), AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                    let raw_id: String = row.get(0)?;
                    let id = AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
                    let name: String = row.get(1)?;
                    let count: i64 = row.get(2)?;
                    let count = usize::try_from(count).map_err(|_| invalid_integer_column(2, "author book count must not be negative"))?;
                    authors.push((id, name, count));
                    Ok(())
                })?;
                authors
            }
            .into_iter()
            .map(|(id, name, book_count)| {
                let books = shelves.remove(&id).unwrap_or_default();
                AuthorSummary::try_new(id, name, book_count, books).map_err(|error| invalid_text_column(0, error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
            sort.sort(&mut authors);
            Ok(authors)
        })
    })
    .expect("authors")
    .is_empty());
}

fn inspected(title: &str, author: &str) -> book_metadata::InspectedBook {
    book_metadata::InspectedBook {
        pdf: None,
        metadata: book_model::BookRecord {
            metadata: book_model::BookDetails { title: title.to_owned(), subtitle: None, contributors: vec![book_model::Contributor::new(author, book_model::MarcRelatorCode(*b"aut")).unwrap()], book: book_model::BookMetadata::default() },
            description: "A synopsis.".to_owned(),
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
        let mut query = page_query(library_model::BrowseLocation::Folder(sync_common::ROOT_DIR_ID));
        query.search = text.into();
        database.browse_contents(&query, false).expect("search").books.iter().map(|card| card.content_hash).collect::<Vec<_>>()
    };
    assert_eq!(search("rust"), [rust]);
    assert_eq!(search("lovelace"), [cooking]);
    assert!(search("missing").is_empty());
}

fn page_query(location: library_model::BrowseLocation) -> library_model::LibraryBrowseQuery {
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
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };
    for subject in [false, true] {
        let holder = open_test_database();
        let db = &holder.database;
        let directory = db.create_directory(&sync_common::ROOT_DIR_ID, &"Original folder".into()).unwrap().id;
        commit_book(db, &directory, "1", "Original book", "Writer", "book.epub");
        db.connection.execute("INSERT INTO book_language SELECT row_id,'en' FROM book", []).unwrap();
        db.connection.execute("INSERT INTO book_unified_concept SELECT row_id,(SELECT concept_id FROM curated.unified_concept_route WHERE concept_id>0 LIMIT 1),1 FROM book", []).unwrap();
        let query = page_query(if subject { library_model::BrowseLocation::Subject(0) } else { library_model::BrowseLocation::Folder(directory) });
        let browse = || db.browse(&query, false);
        // Build membership first so the hook only intercepts a clean read.
        browse().unwrap();
        let fired = Arc::new(AtomicBool::new(false));
        let observed = fired.clone();
        let mutation = Arc::new(Mutex::new(None));
        let result = mutation.clone();
        let path = holder.path.clone();
        db.connection
            .authorizer(Some(move |context: AuthContext<'_>| {
                if matches!(context.action, AuthAction::Read { table_name: "book", column_name: "title" }) && !observed.swap(true, Ordering::SeqCst) {
                    let update = {
                        'update: {
                            let writer = (match Database::open(&path) {
                                Ok(value) => value,
                                Err(error) => break 'update Err(error.into()),
                            });
                            (match writer.connection.execute_batch(" UPDATE book SET title='Changed book',format=2; UPDATE book_language SET language='sv';") {
                                Ok(value) => value,
                                Err(error) => break 'update Err(error.into()),
                            });
                            (match writer.connection.execute("UPDATE dir SET name='Changed folder' WHERE id=?1", [directory.to_string()]) {
                                Ok(value) => value,
                                Err(error) => break 'update Err(error.into()),
                            });
                            Ok::<_, crate::DatabaseError>(())
                        }
                    };
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
    let facts = db.connection.prepare("SELECT content_hash,row_id FROM book").unwrap().query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    let tx = db.connection.unchecked_transaction().unwrap();
    for (hash, id) in facts {
        crate::sync::apply::registers::record_local(
            &tx,
            library_replica::LibraryStateEntry::BookFacts { content_hash: sync_common::ContentHash::new(&hash), value: library_replica::BookFacts { added_at: u64::try_from(id * 1000).unwrap(), format: book_model::BookFormat::Epub } },
        )
        .unwrap();
    }
    crate::sync::apply::commit(tx).unwrap();
    db.connection.execute("INSERT INTO book_unified_concept SELECT row_id,(SELECT concept_id FROM curated.unified_concept_route WHERE concept_id>0 LIMIT 1),1 FROM book", []).unwrap();

    db.update_reading_position(&hash, "epubcfi(/6/2!/4/2/1:0)", Some(25.), None).unwrap();
    db.connection.execute("INSERT OR IGNORE INTO book_unified_concept SELECT row_id,(SELECT concept_id FROM curated.unified_concept_route WHERE concept_id>0 LIMIT 1),1 FROM book", []).unwrap();
    let expected = ({
        use crate::browse::rows::{invalid_integer_column, invalid_text_column};
        use crate::browse::DetailSql;
        use crate::{entry_title, rusqlite, Database, DatabaseError, TocSql};
        use book_model::{AuthorId, AUTHOR_MARC_RELATOR_CODE};
        use library_model::{BookCardRow, BookDetail, BookDetailAuthor};
        use sync_common::ContentHash;
        struct SourceLocation {
            source_directory_id: Option<String>,
            path: Vec<library_model::BrowsePathSegment>,
        }
        let database: &Database = &db;
        let content_hash = hash;

        database.with_browse_snapshot(|| {
            let (book, audiobook_chapter_count) = {
                let content_hash: &ContentHash = &content_hash;
                let mut book = None;
                database.connection.book_detail_book(content_hash.as_str(), |row| {
                    let chapter_count: Option<i64> = row.get(1)?;
                    let chapter_count = chapter_count.map(usize::try_from).transpose().map_err(|error| invalid_integer_column(1, error.to_string()))?;
                    book = Some((row.get::<_, i64>(0)?, chapter_count));
                    Ok(())
                })?;
                let (id, chapter_count) = book.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                let card = database.load_cards(&[id])?.pop().ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                (card, chapter_count)
            };
            let description = {
                let content_hash: &ContentHash = &content_hash;
                let mut description = String::new();
                database.connection.book_detail_description(content_hash.as_str(), |row| {
                    description = row.get(0)?;
                    Ok(())
                })?;
                description
            };
            let SourceLocation { source_directory_id, path } = {
                let content_hash: &ContentHash = &content_hash;
                let mut source_directory_id: Option<String> = None;
                database.connection.book_detail_directory(content_hash.as_str(), |row| {
                    source_directory_id = row.get(0)?;
                    Ok(())
                })?;

                let path: Vec<library_model::BrowsePathSegment> = source_directory_id
                    .as_deref()
                    .map(|directory| {
                        let directory = sync_common::DirId::parse_str(directory).map_err(DatabaseError::operation)?;
                        database.dir_path_components(&directory)
                    })
                    .transpose()?
                    .unwrap_or_default()
                    .into_iter()
                    .map(|component| library_model::BrowsePathSegment { id: library_model::BrowseLocation::Folder(component.id), name: component.name.clone() })
                    .collect();

                SourceLocation { source_directory_id, path }
            };
            let authors = {
                let content_hash: &ContentHash = &content_hash;
                let mut authors = Vec::new();
                database.connection.book_detail_authors(content_hash.as_str(), AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                    let id: String = row.get(0)?;
                    authors.push(BookDetailAuthor { id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?, name: row.get(1)? });
                    Ok(())
                })?;

                authors
            };
            let credits = {
                let content_hash: &ContentHash = &content_hash;
                // Only narrator and translator: a reader picks an audiobook by
                // its narrator and an edition by its translator, and nothing else in the
                // contributor list answers a question a reader is asking.
                let mut credits = Vec::new();
                let narrator = book_model::NARRATOR_MARC_RELATOR_CODE.0.to_vec();
                let translator = book_model::TRANSLATOR_MARC_RELATOR_CODE.0.to_vec();
                database.connection.book_detail_credits(content_hash.as_str(), narrator, translator, |row| {
                    let id: String = row.get(0)?;
                    let role: Vec<u8> = row.get(2)?;
                    credits.push(library_model::BookDetailCredit {
                        id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?,
                        name: row.get(1)?,
                        role: String::from_utf8(role).map_err(|error| invalid_text_column(2, error.to_string()))?,
                    });
                    Ok(())
                })?;

                credits
            };
            let toc = {
                let content_hash: &ContentHash = &content_hash;
                let mut stored: Option<String> = None;
                database.connection.book_toc_select(content_hash.as_str(), |row| {
                    stored = row.get(0)?;
                    Ok(())
                })?;
                let mut entries =
                    stored.map(|json| serde_json::from_str::<Vec<library_model::BookTocEntry>>(&json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}")))).transpose()?.unwrap_or_default();
                for (index, entry) in entries.iter_mut().enumerate() {
                    if book_model::audiobook_toc_position(&entry.target).is_some() {
                        entry.title = book_model::audiobook_chapter_display_title(&entry.title, index + 1);
                    }
                }
                entries
            };
            let (current_entry, current_target) = {
                let content_hash: &ContentHash = &content_hash;
                let format: library_model::LibraryFileTypeFilter = book.format_category;
                let toc: &[library_model::BookTocEntry] = &toc;
                'expected: {
                    if format == library_model::LibraryFileTypeFilter::Audiobook {
                        let Some(position) = database.audiobook_reading_position(content_hash)? else { break 'expected (None, None) };
                        let chapter = ({
                            use crate::{chapters_from_navigation, AudiobookSql};
                            use library_model::{AudiobookChapter, BookTocEntry};
                            let hash: &sync_common::ContentHash = content_hash;

                            'chapter: {
                                let mut stored: Option<(String, i64)> = None;
                                database.connection.audiobook_chapters_select(hash.as_str(), |row| {
                                    stored = Some((row.get(0)?, row.get(1)?));
                                    Ok(())
                                })?;
                                let Some((toc_json, duration_ms)) = stored else { break 'chapter Vec::new() };
                                let entries: Vec<BookTocEntry> = serde_json::from_str(&toc_json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}")))?;
                                chapters_from_navigation(&entries, duration_ms.max(0) as u64)
                            }
                        })
                        .into_iter()
                        .find(|chapter| position >= chapter.start_ms && position < chapter.end_ms);
                        break 'expected match chapter {
                            Some(chapter) => (Some(chapter.title), Some(book_model::audiobook_toc_target(chapter.start_ms))),
                            None => (None, None),
                        };
                    }
                    let mut target: Option<String> = None;
                    database.connection.reading_entry_select(content_hash.as_str(), |row| {
                        target = row.get(0)?;
                        Ok(())
                    })?;
                    let Some(target) = target else { break 'expected (None, None) };
                    (entry_title(toc, &target), Some(target))
                }
            };
            Ok(BookDetail { book, audiobook_chapter_count, description, source_directory_id, path, authors, credits, current_entry, current_target, toc })
        })
    })
    .unwrap()
    .book;
    assert_eq!(expected.added_at, 1000);
    let home = ({
        use crate::browse::{CountsSql, HomeSql};
        use crate::DatabaseError;
        use library_model::LibraryHomeView;
        let database = &db;
        let limit = 10;
        let downloaded_only = false;
        'home: {
            if limit < 0 {
                break 'home Err(DatabaseError::message("browse home limit must not be negative"));
            }
            let downloaded_only = i32::from(downloaded_only);
            database.with_browse_snapshot(|| {
                // The most recently read books: up to `limit` still in progress, then
                // up to `limit` finished. The two groups arrive in one statement and
                // stay in reading order within each, so a caller splits them by the
                // same threshold the query ranked them with.
                let mut read = Vec::new();
                database.connection.recently_read_books(limit, downloaded_only, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD), |row| {
                    read.push(row.get(0)?);
                    Ok(())
                })?;
                let mut added = Vec::new();
                database.connection.recently_added_books(limit, downloaded_only, |row| {
                    added.push(row.get(0)?);
                    Ok(())
                })?;
                Ok(LibraryHomeView { recently_read: database.load_cards(&read)?, recently_added: database.load_cards(&added)? })
            })
        }
    })
    .unwrap();
    let authors = ({
        use crate::browse::rows::{invalid_integer_column, invalid_text_column};
        use crate::browse::AuthorsSql;
        use crate::{Database, DatabaseError};
        use book_model::{AgentId, AuthorId, AUTHOR_MARC_RELATOR_CODE};
        use library_model::{AuthorSummary, BookCardRow};
        use std::collections::HashMap;
        const AUTHOR_SHELF_BOOK_COUNT: i32 = 40;
        let database: &Database = &db;
        let sort = library_model::AuthorSort::Name;
        let downloaded_only = false;

        database.with_browse_snapshot(|| {
            let mut shelves = {
                let per_author: i32 = AUTHOR_SHELF_BOOK_COUNT;
                let (mut authors, mut ids) = (Vec::new(), Vec::new());
                database.connection.author_shelf_books(i32::from(downloaded_only), per_author, AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                    let raw_id: String = row.get(0)?;
                    authors.push(AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?);
                    ids.push(row.get(1)?);
                    Ok(())
                })?;
                let mut shelves: HashMap<AuthorId, Vec<BookCardRow>> = HashMap::new();
                for (author, card) in authors.into_iter().zip(database.load_cards(&ids)?) {
                    shelves.entry(author).or_default().push(card);
                }
                shelves
            };
            let mut authors = {
                let mut authors = Vec::new();
                database.connection.get_authors(i32::from(downloaded_only), AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                    let raw_id: String = row.get(0)?;
                    let id = AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
                    let name: String = row.get(1)?;
                    let count: i64 = row.get(2)?;
                    let count = usize::try_from(count).map_err(|_| invalid_integer_column(2, "author book count must not be negative"))?;
                    authors.push((id, name, count));
                    Ok(())
                })?;
                authors
            }
            .into_iter()
            .map(|(id, name, book_count)| {
                let books = shelves.remove(&id).unwrap_or_default();
                AuthorSummary::try_new(id, name, book_count, books).map_err(|error| invalid_text_column(0, error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
            sort.sort(&mut authors);
            Ok(authors)
        })
    })
    .unwrap();
    let subject = db.browse_contents(&page_query(library_model::BrowseLocation::Subject(0)), false).unwrap();
    for cards in [&home.recently_read, &home.recently_added, &authors[0].books, &subject.books] {
        assert_eq!(cards.iter().find(|card| card.content_hash == hash).unwrap(), &expected);
    }
    let mut query = page_query(library_model::BrowseLocation::Folder(directory));
    query.file_types = vec![LibraryFileTypeFilter::Book];
    let folder = db.browse_contents(&query, false).unwrap();
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

    db.connection.execute("UPDATE book SET format=2 WHERE content_hash=?1", [pdf.as_str()]).unwrap();
    db.connection.execute("INSERT INTO book_unified_concept SELECT row_id,(SELECT concept_id FROM curated.unified_concept_route WHERE concept_id>0 LIMIT 1),1 FROM book WHERE content_hash=?1", [pdf.as_str()]).unwrap();
    let search = |location: library_model::BrowseLocation, file_type: LibraryFileTypeFilter| {
        let mut query = page_query(location);
        query.search = "shared".into();
        query.file_types = vec![file_type];
        db.browse_contents(&query, false).unwrap().books.iter().map(|card| card.content_hash).collect::<Vec<_>>()
    };
    assert_eq!(search(library_model::BrowseLocation::Folder(shelf), LibraryFileTypeFilter::Pdf), [pdf]);
    assert_eq!(search(library_model::BrowseLocation::Folder(shelf), LibraryFileTypeFilter::Book), [epub]);
    assert_eq!(search(library_model::BrowseLocation::Subject(0), LibraryFileTypeFilter::All), [pdf]);
    assert!(search(library_model::BrowseLocation::Subject(0), LibraryFileTypeFilter::Book).is_empty());
}

#[test]
fn blank_language_selections_and_home_limits() {
    let holder = open_test_database();
    let db = &holder.database;
    let hash = commit_book(db, &sync_common::ROOT_DIR_ID, "7", "Book", "Author", "book.epub");
    db.update_reading_position(&hash, "epubcfi(/6/2!/4/2/1:0)", Some(25.), None).unwrap();
    db.connection.execute("INSERT INTO book_language SELECT row_id,'en' FROM book", []).unwrap();
    db.connection.execute("INSERT INTO book_unified_concept SELECT row_id,(SELECT concept_id FROM curated.unified_concept_route WHERE concept_id>0 LIMIT 1),1 FROM book", []).unwrap();
    for location in [library_model::BrowseLocation::Folder(sync_common::ROOT_DIR_ID), library_model::BrowseLocation::Subject(0)] {
        let mut query = page_query(location);
        let browse = |query: &library_model::LibraryBrowseQuery| db.browse(query, false).unwrap();
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
        assert!(({
            use crate::browse::{CountsSql, HomeSql};
            use crate::DatabaseError;
            use library_model::LibraryHomeView;
            let database = &db;
            let limit = limit;
            let downloaded_only = false;
            'blank_language_selections_and_home_limits: {
                if limit < 0 {
                    break 'blank_language_selections_and_home_limits Err(DatabaseError::message("browse home limit must not be negative"));
                }
                let downloaded_only = i32::from(downloaded_only);
                database.with_browse_snapshot(|| {
                    // The most recently read books: up to `limit` still in progress, then
                    // up to `limit` finished. The two groups arrive in one statement and
                    // stay in reading order within each, so a caller splits them by the
                    // same threshold the query ranked them with.
                    let mut read = Vec::new();
                    database.connection.recently_read_books(limit, downloaded_only, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD), |row| {
                        read.push(row.get(0)?);
                        Ok(())
                    })?;
                    let mut added = Vec::new();
                    database.connection.recently_added_books(limit, downloaded_only, |row| {
                        added.push(row.get(0)?);
                        Ok(())
                    })?;
                    Ok(LibraryHomeView { recently_read: database.load_cards(&read)?, recently_added: database.load_cards(&added)? })
                })
            }
        })
        .unwrap_err()
        .to_string()
        .contains("limit"));
    }
    for limit in [0, 1] {
        let view = ({
            use crate::browse::{CountsSql, HomeSql};
            use crate::DatabaseError;
            use library_model::LibraryHomeView;
            let database = &db;
            let limit = limit;
            let downloaded_only = false;
            'view: {
                if limit < 0 {
                    break 'view Err(DatabaseError::message("browse home limit must not be negative"));
                }
                let downloaded_only = i32::from(downloaded_only);
                database.with_browse_snapshot(|| {
                    // The most recently read books: up to `limit` still in progress, then
                    // up to `limit` finished. The two groups arrive in one statement and
                    // stay in reading order within each, so a caller splits them by the
                    // same threshold the query ranked them with.
                    let mut read = Vec::new();
                    database.connection.recently_read_books(limit, downloaded_only, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD), |row| {
                        read.push(row.get(0)?);
                        Ok(())
                    })?;
                    let mut added = Vec::new();
                    database.connection.recently_added_books(limit, downloaded_only, |row| {
                        added.push(row.get(0)?);
                        Ok(())
                    })?;
                    Ok(LibraryHomeView { recently_read: database.load_cards(&read)?, recently_added: database.load_cards(&added)? })
                })
            }
        })
        .unwrap();
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
    for (view, table, column) in [("home", "book", "added_at"), ("authors", "book_contributor", "name"), ("detail", "book_dir", "dir_id")] {
        let holder = open_test_database();
        let db = &holder.database;
        let directory = db.create_directory(&sync_common::ROOT_DIR_ID, &"Original folder".into()).unwrap().id;
        let hash = commit_book(db, &directory, "8", "Original book", "Author", "book.epub");
        db.update_reading_position(&hash, "epubcfi(/6/2!/4/2/1:0)", Some(25.), None).unwrap();
        db.connection.execute("INSERT OR IGNORE INTO book_unified_concept SELECT row_id,(SELECT concept_id FROM curated.unified_concept_route WHERE concept_id>0 LIMIT 1),1 FROM book", []).unwrap();
        let outcome = Arc::new(Mutex::new(None));
        let observed = outcome.clone();
        let path = holder.path.clone();
        db.connection
            .authorizer(Some(move |context: AuthContext<'_>| {
                if let AuthAction::Read { table_name, column_name } = context.action {
                    let mut result = observed.lock().unwrap();
                    if table_name == table && column_name == column && result.is_none() {
                        *result = Some(
                            {
                                'home_authors_and_detail_hold_a_snapshot_between_component_queries: {
                                    let writer = (match Database::open(&path) {
                                        Ok(value) => value,
                                        Err(error) => break 'home_authors_and_detail_hold_a_snapshot_between_component_queries Err(error.into()),
                                    });
                                    (match writer.connection.execute_batch(" UPDATE book SET title='Changed'; UPDATE book_dir SET deleted_at=1;") {
                                        Ok(value) => value,
                                        Err(error) => break 'home_authors_and_detail_hold_a_snapshot_between_component_queries Err(error.into()),
                                    });
                                    Ok::<_, crate::DatabaseError>(())
                                }
                            }
                            .map_err(|error| error.to_string()),
                        );
                    }
                }
                Authorization::Allow
            }))
            .unwrap();
        match view {
            "home" => {
                let home = ({
                    use crate::browse::{CountsSql, HomeSql};
                    use crate::DatabaseError;
                    use library_model::LibraryHomeView;
                    let database = &db;
                    let limit = 10;
                    let downloaded_only = false;
                    'home: {
                        if limit < 0 {
                            break 'home Err(DatabaseError::message("browse home limit must not be negative"));
                        }
                        let downloaded_only = i32::from(downloaded_only);
                        database.with_browse_snapshot(|| {
                            // The most recently read books: up to `limit` still in progress, then
                            // up to `limit` finished. The two groups arrive in one statement and
                            // stay in reading order within each, so a caller splits them by the
                            // same threshold the query ranked them with.
                            let mut read = Vec::new();
                            database.connection.recently_read_books(limit, downloaded_only, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD), |row| {
                                read.push(row.get(0)?);
                                Ok(())
                            })?;
                            let mut added = Vec::new();
                            database.connection.recently_added_books(limit, downloaded_only, |row| {
                                added.push(row.get(0)?);
                                Ok(())
                            })?;
                            Ok(LibraryHomeView { recently_read: database.load_cards(&read)?, recently_added: database.load_cards(&added)? })
                        })
                    }
                })
                .unwrap();
                for cards in [home.recently_read, home.recently_added] {
                    assert_eq!(cards.len(), 1);
                    assert_eq!(cards[0].title, "Original book");
                }
            }
            "authors" => {
                let authors = ({
                    use crate::browse::rows::{invalid_integer_column, invalid_text_column};
                    use crate::browse::AuthorsSql;
                    use crate::{Database, DatabaseError};
                    use book_model::{AgentId, AuthorId, AUTHOR_MARC_RELATOR_CODE};
                    use library_model::{AuthorSummary, BookCardRow};
                    use std::collections::HashMap;
                    const AUTHOR_SHELF_BOOK_COUNT: i32 = 40;
                    let database: &Database = &db;
                    let sort = library_model::AuthorSort::Name;
                    let downloaded_only = false;

                    database.with_browse_snapshot(|| {
                        let mut shelves = {
                            let per_author: i32 = AUTHOR_SHELF_BOOK_COUNT;
                            let (mut authors, mut ids) = (Vec::new(), Vec::new());
                            database.connection.author_shelf_books(i32::from(downloaded_only), per_author, AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                                let raw_id: String = row.get(0)?;
                                authors.push(AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?);
                                ids.push(row.get(1)?);
                                Ok(())
                            })?;
                            let mut shelves: HashMap<AuthorId, Vec<BookCardRow>> = HashMap::new();
                            for (author, card) in authors.into_iter().zip(database.load_cards(&ids)?) {
                                shelves.entry(author).or_default().push(card);
                            }
                            shelves
                        };
                        let mut authors = {
                            let mut authors = Vec::new();
                            database.connection.get_authors(i32::from(downloaded_only), AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                                let raw_id: String = row.get(0)?;
                                let id = AgentId::parse_str(&raw_id).map_err(|error| invalid_text_column(0, error.to_string()))?;
                                let name: String = row.get(1)?;
                                let count: i64 = row.get(2)?;
                                let count = usize::try_from(count).map_err(|_| invalid_integer_column(2, "author book count must not be negative"))?;
                                authors.push((id, name, count));
                                Ok(())
                            })?;
                            authors
                        }
                        .into_iter()
                        .map(|(id, name, book_count)| {
                            let books = shelves.remove(&id).unwrap_or_default();
                            AuthorSummary::try_new(id, name, book_count, books).map_err(|error| invalid_text_column(0, error.to_string()))
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                        sort.sort(&mut authors);
                        Ok(authors)
                    })
                })
                .unwrap();
                assert_eq!(authors.len(), 1);
                assert_eq!(authors[0].book_count, 1);
                assert_eq!(authors[0].books[0].title, "Original book");
            }
            _ => {
                let detail = ({
                    use crate::browse::rows::{invalid_integer_column, invalid_text_column};
                    use crate::browse::DetailSql;
                    use crate::{entry_title, rusqlite, Database, DatabaseError, TocSql};
                    use book_model::{AuthorId, AUTHOR_MARC_RELATOR_CODE};
                    use library_model::{BookCardRow, BookDetail, BookDetailAuthor};
                    use sync_common::ContentHash;
                    struct SourceLocation {
                        source_directory_id: Option<String>,
                        path: Vec<library_model::BrowsePathSegment>,
                    }
                    let database: &Database = &db;
                    let content_hash = hash;

                    database.with_browse_snapshot(|| {
                        let (book, audiobook_chapter_count) = {
                            let content_hash: &ContentHash = &content_hash;
                            let mut book = None;
                            database.connection.book_detail_book(content_hash.as_str(), |row| {
                                let chapter_count: Option<i64> = row.get(1)?;
                                let chapter_count = chapter_count.map(usize::try_from).transpose().map_err(|error| invalid_integer_column(1, error.to_string()))?;
                                book = Some((row.get::<_, i64>(0)?, chapter_count));
                                Ok(())
                            })?;
                            let (id, chapter_count) = book.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                            let card = database.load_cards(&[id])?.pop().ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                            (card, chapter_count)
                        };
                        let description = {
                            let content_hash: &ContentHash = &content_hash;
                            let mut description = String::new();
                            database.connection.book_detail_description(content_hash.as_str(), |row| {
                                description = row.get(0)?;
                                Ok(())
                            })?;
                            description
                        };
                        let SourceLocation { source_directory_id, path } = {
                            let content_hash: &ContentHash = &content_hash;
                            let mut source_directory_id: Option<String> = None;
                            database.connection.book_detail_directory(content_hash.as_str(), |row| {
                                source_directory_id = row.get(0)?;
                                Ok(())
                            })?;

                            let path: Vec<library_model::BrowsePathSegment> = source_directory_id
                                .as_deref()
                                .map(|directory| {
                                    let directory = sync_common::DirId::parse_str(directory).map_err(DatabaseError::operation)?;
                                    database.dir_path_components(&directory)
                                })
                                .transpose()?
                                .unwrap_or_default()
                                .into_iter()
                                .map(|component| library_model::BrowsePathSegment { id: library_model::BrowseLocation::Folder(component.id), name: component.name.clone() })
                                .collect();

                            SourceLocation { source_directory_id, path }
                        };
                        let authors = {
                            let content_hash: &ContentHash = &content_hash;
                            let mut authors = Vec::new();
                            database.connection.book_detail_authors(content_hash.as_str(), AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                                let id: String = row.get(0)?;
                                authors.push(BookDetailAuthor { id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?, name: row.get(1)? });
                                Ok(())
                            })?;

                            authors
                        };
                        let credits = {
                            let content_hash: &ContentHash = &content_hash;
                            // Only narrator and translator: a reader picks an audiobook by
                            // its narrator and an edition by its translator, and nothing else in the
                            // contributor list answers a question a reader is asking.
                            let mut credits = Vec::new();
                            let narrator = book_model::NARRATOR_MARC_RELATOR_CODE.0.to_vec();
                            let translator = book_model::TRANSLATOR_MARC_RELATOR_CODE.0.to_vec();
                            database.connection.book_detail_credits(content_hash.as_str(), narrator, translator, |row| {
                                let id: String = row.get(0)?;
                                let role: Vec<u8> = row.get(2)?;
                                credits.push(library_model::BookDetailCredit {
                                    id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?,
                                    name: row.get(1)?,
                                    role: String::from_utf8(role).map_err(|error| invalid_text_column(2, error.to_string()))?,
                                });
                                Ok(())
                            })?;

                            credits
                        };
                        let toc = {
                            let content_hash: &ContentHash = &content_hash;
                            let mut stored: Option<String> = None;
                            database.connection.book_toc_select(content_hash.as_str(), |row| {
                                stored = row.get(0)?;
                                Ok(())
                            })?;
                            let mut entries = stored
                                .map(|json| serde_json::from_str::<Vec<library_model::BookTocEntry>>(&json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}"))))
                                .transpose()?
                                .unwrap_or_default();
                            for (index, entry) in entries.iter_mut().enumerate() {
                                if book_model::audiobook_toc_position(&entry.target).is_some() {
                                    entry.title = book_model::audiobook_chapter_display_title(&entry.title, index + 1);
                                }
                            }
                            entries
                        };
                        let (current_entry, current_target) = {
                            let content_hash: &ContentHash = &content_hash;
                            let format: library_model::LibraryFileTypeFilter = book.format_category;
                            let toc: &[library_model::BookTocEntry] = &toc;
                            'detail: {
                                if format == library_model::LibraryFileTypeFilter::Audiobook {
                                    let Some(position) = database.audiobook_reading_position(content_hash)? else { break 'detail (None, None) };
                                    let chapter = ({
                                        use crate::{chapters_from_navigation, AudiobookSql};
                                        use library_model::{AudiobookChapter, BookTocEntry};
                                        let hash: &sync_common::ContentHash = content_hash;

                                        'chapter: {
                                            let mut stored: Option<(String, i64)> = None;
                                            database.connection.audiobook_chapters_select(hash.as_str(), |row| {
                                                stored = Some((row.get(0)?, row.get(1)?));
                                                Ok(())
                                            })?;
                                            let Some((toc_json, duration_ms)) = stored else { break 'chapter Vec::new() };
                                            let entries: Vec<BookTocEntry> = serde_json::from_str(&toc_json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}")))?;
                                            chapters_from_navigation(&entries, duration_ms.max(0) as u64)
                                        }
                                    })
                                    .into_iter()
                                    .find(|chapter| position >= chapter.start_ms && position < chapter.end_ms);
                                    break 'detail match chapter {
                                        Some(chapter) => (Some(chapter.title), Some(book_model::audiobook_toc_target(chapter.start_ms))),
                                        None => (None, None),
                                    };
                                }
                                let mut target: Option<String> = None;
                                database.connection.reading_entry_select(content_hash.as_str(), |row| {
                                    target = row.get(0)?;
                                    Ok(())
                                })?;
                                let Some(target) = target else { break 'detail (None, None) };
                                (entry_title(toc, &target), Some(target))
                            }
                        };
                        Ok(BookDetail { book, audiobook_chapter_count, description, source_directory_id, path, authors, credits, current_entry, current_target, toc })
                    })
                })
                .unwrap();
                assert_eq!(detail.book.title, "Original book");
                assert_eq!(detail.source_directory_id, Some(directory.to_string()));
                assert_eq!(detail.path.last().unwrap().name, "Original folder");
            }
        }
        db.connection.authorizer(None::<fn(AuthContext<'_>) -> Authorization>).unwrap();
        assert_eq!(outcome.lock().unwrap().as_ref(), Some(&Ok(())), "{view}: writer ran between component queries");
        assert_eq!(
            ({
                use crate::browse::{CountsSql, HomeSql};
                use crate::DatabaseError;
                use library_model::LibraryHomeView;
                let database = &db;
                'home_authors_and_detail_hold_a_snapshot_between_component_queries: {
                    let mut count = 0_i64;
                    (match database.connection.count_all_books(|row| {
                        count = row.get(0)?;
                        Ok(())
                    }) {
                        Ok(value) => value,
                        Err(error) => break 'home_authors_and_detail_hold_a_snapshot_between_component_queries Err(error.into()),
                    });
                    Ok::<_, DatabaseError>(
                        (match usize::try_from(count).map_err(|_| DatabaseError::message("library book count must not be negative")) {
                            Ok(value) => value,
                            Err(error) => break 'home_authors_and_detail_hold_a_snapshot_between_component_queries Err(error.into()),
                        }),
                    )
                }
            })
            .unwrap(),
            0,
            "{view}: snapshot released"
        );
        assert!(db.connection.is_autocommit());
    }
}

#[test]
fn failed_view_read_releases_its_snapshot() {
    let holder = open_test_database();
    let db = &holder.database;
    let missing = sync_common::ContentHash::new(&"f".repeat(64));
    assert!(({
        use crate::browse::rows::{invalid_integer_column, invalid_text_column};
        use crate::browse::DetailSql;
        use crate::{entry_title, rusqlite, Database, DatabaseError, TocSql};
        use book_model::{AuthorId, AUTHOR_MARC_RELATOR_CODE};
        use library_model::{BookCardRow, BookDetail, BookDetailAuthor};
        use sync_common::ContentHash;
        struct SourceLocation {
            source_directory_id: Option<String>,
            path: Vec<library_model::BrowsePathSegment>,
        }
        let database: &Database = &db;
        let content_hash = missing;

        database.with_browse_snapshot(|| {
            let (book, audiobook_chapter_count) = {
                let content_hash: &ContentHash = &content_hash;
                let mut book = None;
                database.connection.book_detail_book(content_hash.as_str(), |row| {
                    let chapter_count: Option<i64> = row.get(1)?;
                    let chapter_count = chapter_count.map(usize::try_from).transpose().map_err(|error| invalid_integer_column(1, error.to_string()))?;
                    book = Some((row.get::<_, i64>(0)?, chapter_count));
                    Ok(())
                })?;
                let (id, chapter_count) = book.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                let card = database.load_cards(&[id])?.pop().ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                (card, chapter_count)
            };
            let description = {
                let content_hash: &ContentHash = &content_hash;
                let mut description = String::new();
                database.connection.book_detail_description(content_hash.as_str(), |row| {
                    description = row.get(0)?;
                    Ok(())
                })?;
                description
            };
            let SourceLocation { source_directory_id, path } = {
                let content_hash: &ContentHash = &content_hash;
                let mut source_directory_id: Option<String> = None;
                database.connection.book_detail_directory(content_hash.as_str(), |row| {
                    source_directory_id = row.get(0)?;
                    Ok(())
                })?;

                let path: Vec<library_model::BrowsePathSegment> = source_directory_id
                    .as_deref()
                    .map(|directory| {
                        let directory = sync_common::DirId::parse_str(directory).map_err(DatabaseError::operation)?;
                        database.dir_path_components(&directory)
                    })
                    .transpose()?
                    .unwrap_or_default()
                    .into_iter()
                    .map(|component| library_model::BrowsePathSegment { id: library_model::BrowseLocation::Folder(component.id), name: component.name.clone() })
                    .collect();

                SourceLocation { source_directory_id, path }
            };
            let authors = {
                let content_hash: &ContentHash = &content_hash;
                let mut authors = Vec::new();
                database.connection.book_detail_authors(content_hash.as_str(), AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                    let id: String = row.get(0)?;
                    authors.push(BookDetailAuthor { id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?, name: row.get(1)? });
                    Ok(())
                })?;

                authors
            };
            let credits = {
                let content_hash: &ContentHash = &content_hash;
                // Only narrator and translator: a reader picks an audiobook by
                // its narrator and an edition by its translator, and nothing else in the
                // contributor list answers a question a reader is asking.
                let mut credits = Vec::new();
                let narrator = book_model::NARRATOR_MARC_RELATOR_CODE.0.to_vec();
                let translator = book_model::TRANSLATOR_MARC_RELATOR_CODE.0.to_vec();
                database.connection.book_detail_credits(content_hash.as_str(), narrator, translator, |row| {
                    let id: String = row.get(0)?;
                    let role: Vec<u8> = row.get(2)?;
                    credits.push(library_model::BookDetailCredit {
                        id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?,
                        name: row.get(1)?,
                        role: String::from_utf8(role).map_err(|error| invalid_text_column(2, error.to_string()))?,
                    });
                    Ok(())
                })?;

                credits
            };
            let toc = {
                let content_hash: &ContentHash = &content_hash;
                let mut stored: Option<String> = None;
                database.connection.book_toc_select(content_hash.as_str(), |row| {
                    stored = row.get(0)?;
                    Ok(())
                })?;
                let mut entries =
                    stored.map(|json| serde_json::from_str::<Vec<library_model::BookTocEntry>>(&json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}")))).transpose()?.unwrap_or_default();
                for (index, entry) in entries.iter_mut().enumerate() {
                    if book_model::audiobook_toc_position(&entry.target).is_some() {
                        entry.title = book_model::audiobook_chapter_display_title(&entry.title, index + 1);
                    }
                }
                entries
            };
            let (current_entry, current_target) = {
                let content_hash: &ContentHash = &content_hash;
                let format: library_model::LibraryFileTypeFilter = book.format_category;
                let toc: &[library_model::BookTocEntry] = &toc;
                'failed_view_read_releases_its_snapshot: {
                    if format == library_model::LibraryFileTypeFilter::Audiobook {
                        let Some(position) = database.audiobook_reading_position(content_hash)? else { break 'failed_view_read_releases_its_snapshot (None, None) };
                        let chapter = ({
                            use crate::{chapters_from_navigation, AudiobookSql};
                            use library_model::{AudiobookChapter, BookTocEntry};
                            let hash: &sync_common::ContentHash = content_hash;

                            'chapter: {
                                let mut stored: Option<(String, i64)> = None;
                                database.connection.audiobook_chapters_select(hash.as_str(), |row| {
                                    stored = Some((row.get(0)?, row.get(1)?));
                                    Ok(())
                                })?;
                                let Some((toc_json, duration_ms)) = stored else { break 'chapter Vec::new() };
                                let entries: Vec<BookTocEntry> = serde_json::from_str(&toc_json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}")))?;
                                chapters_from_navigation(&entries, duration_ms.max(0) as u64)
                            }
                        })
                        .into_iter()
                        .find(|chapter| position >= chapter.start_ms && position < chapter.end_ms);
                        break 'failed_view_read_releases_its_snapshot match chapter {
                            Some(chapter) => (Some(chapter.title), Some(book_model::audiobook_toc_target(chapter.start_ms))),
                            None => (None, None),
                        };
                    }
                    let mut target: Option<String> = None;
                    database.connection.reading_entry_select(content_hash.as_str(), |row| {
                        target = row.get(0)?;
                        Ok(())
                    })?;
                    let Some(target) = target else { break 'failed_view_read_releases_its_snapshot (None, None) };
                    (entry_title(toc, &target), Some(target))
                }
            };
            Ok(BookDetail { book, audiobook_chapter_count, description, source_directory_id, path, authors, credits, current_entry, current_target, toc })
        })
    })
    .is_err());
    assert!(db.connection.is_autocommit());
    commit_book(db, &sync_common::ROOT_DIR_ID, "b", "Recovered", "Writer", "book.epub");
    assert_eq!(
        ({
            use crate::browse::{CountsSql, HomeSql};
            use crate::DatabaseError;
            use library_model::LibraryHomeView;
            let database = &db;
            let limit = 1;
            let downloaded_only = false;
            'failed_view_read_releases_its_snapshot: {
                if limit < 0 {
                    break 'failed_view_read_releases_its_snapshot Err(DatabaseError::message("browse home limit must not be negative"));
                }
                let downloaded_only = i32::from(downloaded_only);
                database.with_browse_snapshot(|| {
                    // The most recently read books: up to `limit` still in progress, then
                    // up to `limit` finished. The two groups arrive in one statement and
                    // stay in reading order within each, so a caller splits them by the
                    // same threshold the query ranked them with.
                    let mut read = Vec::new();
                    database.connection.recently_read_books(limit, downloaded_only, f64::from(library_model::FINISHED_PROGRESS_THRESHOLD), |row| {
                        read.push(row.get(0)?);
                        Ok(())
                    })?;
                    let mut added = Vec::new();
                    database.connection.recently_added_books(limit, downloaded_only, |row| {
                        added.push(row.get(0)?);
                        Ok(())
                    })?;
                    Ok(LibraryHomeView { recently_read: database.load_cards(&read)?, recently_added: database.load_cards(&added)? })
                })
            }
        })
        .unwrap()
        .recently_added
        .len(),
        1
    );
}

fn assert_untitled_search(db: &Database, hash: sync_common::ContentHash, search: &str, expected_count: usize) {
    for location in [library_model::BrowseLocation::Folder(sync_common::ROOT_DIR_ID), library_model::BrowseLocation::Subject(0)] {
        let mut query = page_query(location);
        query.search = search.into();
        let page = db.browse(&query, false).unwrap();
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

    let hash = sync_common::ContentHash::new(&"c".repeat(64));
    let set_title = |title: Option<&str>, subtitle: Option<&str>| {
        db.connection.execute("UPDATE book SET title=?1,subtitle=?2,search_title=?3", rusqlite::params![title, subtitle, text::search_title(hash.as_str(), title, subtitle)]).unwrap();
    };
    db.connection.execute("INSERT INTO book(content_hash,format) VALUES(?1,1)", [hash.as_str()]).unwrap();
    set_title(None, Some("Écology"));
    db.connection.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) SELECT ?1,row_id,'untitled.epub','',1 FROM book", [sync_common::ROOT_DIR_ID.to_string()]).unwrap();
    db.sync_publishable_mutations().unwrap();

    db.connection.execute("INSERT INTO book_language SELECT row_id,'en' FROM book", []).unwrap();
    db.connection.execute("INSERT INTO book_unified_concept SELECT row_id,(SELECT concept_id FROM curated.unified_concept_route WHERE concept_id>0 LIMIT 1),1 FROM book", []).unwrap();
    for search in ["untitled", hash.as_str(), "ecology"] {
        assert_untitled_search(db, hash, search, 1);
    }
    let display = format!("Untitled ({hash}): Écology");
    assert_untitled_search(db, hash, &display, 1);
    assert_eq!(
        ({
            use crate::browse::rows::{invalid_integer_column, invalid_text_column};
            use crate::browse::DetailSql;
            use crate::{entry_title, rusqlite, Database, DatabaseError, TocSql};
            use book_model::{AuthorId, AUTHOR_MARC_RELATOR_CODE};
            use library_model::{BookCardRow, BookDetail, BookDetailAuthor};
            use sync_common::ContentHash;
            struct SourceLocation {
                source_directory_id: Option<String>,
                path: Vec<library_model::BrowsePathSegment>,
            }
            let database: &Database = &db;
            let content_hash = hash;

            database.with_browse_snapshot(|| {
                let (book, audiobook_chapter_count) = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut book = None;
                    database.connection.book_detail_book(content_hash.as_str(), |row| {
                        let chapter_count: Option<i64> = row.get(1)?;
                        let chapter_count = chapter_count.map(usize::try_from).transpose().map_err(|error| invalid_integer_column(1, error.to_string()))?;
                        book = Some((row.get::<_, i64>(0)?, chapter_count));
                        Ok(())
                    })?;
                    let (id, chapter_count) = book.ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                    let card = database.load_cards(&[id])?.pop().ok_or(rusqlite::Error::QueryReturnedNoRows)?;
                    (card, chapter_count)
                };
                let description = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut description = String::new();
                    database.connection.book_detail_description(content_hash.as_str(), |row| {
                        description = row.get(0)?;
                        Ok(())
                    })?;
                    description
                };
                let SourceLocation { source_directory_id, path } = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut source_directory_id: Option<String> = None;
                    database.connection.book_detail_directory(content_hash.as_str(), |row| {
                        source_directory_id = row.get(0)?;
                        Ok(())
                    })?;

                    let path: Vec<library_model::BrowsePathSegment> = source_directory_id
                        .as_deref()
                        .map(|directory| {
                            let directory = sync_common::DirId::parse_str(directory).map_err(DatabaseError::operation)?;
                            database.dir_path_components(&directory)
                        })
                        .transpose()?
                        .unwrap_or_default()
                        .into_iter()
                        .map(|component| library_model::BrowsePathSegment { id: library_model::BrowseLocation::Folder(component.id), name: component.name.clone() })
                        .collect();

                    SourceLocation { source_directory_id, path }
                };
                let authors = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut authors = Vec::new();
                    database.connection.book_detail_authors(content_hash.as_str(), AUTHOR_MARC_RELATOR_CODE.0.to_vec(), |row| {
                        let id: String = row.get(0)?;
                        authors.push(BookDetailAuthor { id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?, name: row.get(1)? });
                        Ok(())
                    })?;

                    authors
                };
                let credits = {
                    let content_hash: &ContentHash = &content_hash;
                    // Only narrator and translator: a reader picks an audiobook by
                    // its narrator and an edition by its translator, and nothing else in the
                    // contributor list answers a question a reader is asking.
                    let mut credits = Vec::new();
                    let narrator = book_model::NARRATOR_MARC_RELATOR_CODE.0.to_vec();
                    let translator = book_model::TRANSLATOR_MARC_RELATOR_CODE.0.to_vec();
                    database.connection.book_detail_credits(content_hash.as_str(), narrator, translator, |row| {
                        let id: String = row.get(0)?;
                        let role: Vec<u8> = row.get(2)?;
                        credits.push(library_model::BookDetailCredit {
                            id: AuthorId::parse_str(&id).map_err(|error| invalid_text_column(0, error.to_string()))?,
                            name: row.get(1)?,
                            role: String::from_utf8(role).map_err(|error| invalid_text_column(2, error.to_string()))?,
                        });
                        Ok(())
                    })?;

                    credits
                };
                let toc = {
                    let content_hash: &ContentHash = &content_hash;
                    let mut stored: Option<String> = None;
                    database.connection.book_toc_select(content_hash.as_str(), |row| {
                        stored = row.get(0)?;
                        Ok(())
                    })?;
                    let mut entries = stored
                        .map(|json| serde_json::from_str::<Vec<library_model::BookTocEntry>>(&json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}"))))
                        .transpose()?
                        .unwrap_or_default();
                    for (index, entry) in entries.iter_mut().enumerate() {
                        if book_model::audiobook_toc_position(&entry.target).is_some() {
                            entry.title = book_model::audiobook_chapter_display_title(&entry.title, index + 1);
                        }
                    }
                    entries
                };
                let (current_entry, current_target) = {
                    let content_hash: &ContentHash = &content_hash;
                    let format: library_model::LibraryFileTypeFilter = book.format_category;
                    let toc: &[library_model::BookTocEntry] = &toc;
                    'untitled_books_share_search_matches_and_facets_across_views: {
                        if format == library_model::LibraryFileTypeFilter::Audiobook {
                            let Some(position) = database.audiobook_reading_position(content_hash)? else { break 'untitled_books_share_search_matches_and_facets_across_views (None, None) };
                            let chapter = ({
                                use crate::{chapters_from_navigation, AudiobookSql};
                                use library_model::{AudiobookChapter, BookTocEntry};
                                let hash: &sync_common::ContentHash = content_hash;

                                'chapter: {
                                    let mut stored: Option<(String, i64)> = None;
                                    database.connection.audiobook_chapters_select(hash.as_str(), |row| {
                                        stored = Some((row.get(0)?, row.get(1)?));
                                        Ok(())
                                    })?;
                                    let Some((toc_json, duration_ms)) = stored else { break 'chapter Vec::new() };
                                    let entries: Vec<BookTocEntry> = serde_json::from_str(&toc_json).map_err(|error| DatabaseError::message(format!("stored table of contents is not readable: {error}")))?;
                                    chapters_from_navigation(&entries, duration_ms.max(0) as u64)
                                }
                            })
                            .into_iter()
                            .find(|chapter| position >= chapter.start_ms && position < chapter.end_ms);
                            break 'untitled_books_share_search_matches_and_facets_across_views match chapter {
                                Some(chapter) => (Some(chapter.title), Some(book_model::audiobook_toc_target(chapter.start_ms))),
                                None => (None, None),
                            };
                        }
                        let mut target: Option<String> = None;
                        database.connection.reading_entry_select(content_hash.as_str(), |row| {
                            target = row.get(0)?;
                            Ok(())
                        })?;
                        let Some(target) = target else { break 'untitled_books_share_search_matches_and_facets_across_views (None, None) };
                        (entry_title(toc, &target), Some(target))
                    }
                };
                Ok(BookDetail { book, audiobook_chapter_count, description, source_directory_id, path, authors, credits, current_entry, current_target, toc })
            })
        })
        .unwrap()
        .book
        .title,
        format!("Untitled ({hash})")
    );

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

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn shared_presence_tracks_membership_availability_and_book_trash() {
    let holder = open_test_database();
    let db = &holder.database;
    let root = sync_common::ROOT_DIR_ID;
    let folder = db.create_directory(&root, &"Copies".into()).unwrap().id;
    let hash = commit_book(db, &root, "6", "Shared book", "Shared author", "shared.epub");
    db.transfer_book_placement(&hash, &root, &folder, false).unwrap();
    let presence = || db.connection.query_row("SELECT downloaded FROM library_book_presence WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)", [hash.as_str()], |row| row.get::<_, bool>(0)).optional().unwrap();
    assert_eq!(presence(), Some(true));
    let pending = db.native_file_work().unwrap().into_iter().find(|work| work.operation == "materialize").unwrap();
    let snapshot = db.native_file_work_snapshot(pending).unwrap().unwrap();
    db.acknowledge_native_file_work(crate::NativeFileWorkCompletion { final_path: snapshot.resolved_path.to_string(), snapshot, renamed: None, published: true, thumbnail_set_exists: true }).unwrap();
    forget_file(&db.connection, &root.to_string(), &hash).unwrap();
    assert_eq!(presence(), Some(true), "another materialized copy makes the book available");
    db.remove_book_placement(&hash, &folder).unwrap();
    assert_eq!(presence(), Some(true), "retained source bytes remain available until physical removal");
    assert!(db.native_file_work().unwrap().is_empty(), "soft removal retains physical files");
    forget_file(&db.connection, &folder.to_string(), &hash).unwrap();
    assert_eq!(presence(), Some(false), "membership survives removal of the last local file");
    assert_eq!(
        ({
            use crate::browse::{CountsSql, HomeSql};
            use crate::DatabaseError;
            use library_model::LibraryHomeView;
            let database = &db;
            'shared_presence_tracks_membership_availability_and_book_trash: {
                let mut count = 0_i64;
                (match database.connection.count_all_books(|row| {
                    count = row.get(0)?;
                    Ok(())
                }) {
                    Ok(value) => value,
                    Err(error) => break 'shared_presence_tracks_membership_availability_and_book_trash Err(error.into()),
                });
                Ok::<_, DatabaseError>(
                    (match usize::try_from(count).map_err(|_| DatabaseError::message("library book count must not be negative")) {
                        Ok(value) => value,
                        Err(error) => break 'shared_presence_tracks_membership_availability_and_book_trash Err(error.into()),
                    }),
                )
            }
        })
        .unwrap(),
        1
    );
    db.trash_book(&hash).unwrap();
    assert_eq!(presence(), None);
    assert_eq!(
        ({
            use crate::browse::{CountsSql, HomeSql};
            use crate::DatabaseError;
            use library_model::LibraryHomeView;
            let database = &db;
            'shared_presence_tracks_membership_availability_and_book_trash: {
                let mut count = 0_i64;
                (match database.connection.count_all_books(|row| {
                    count = row.get(0)?;
                    Ok(())
                }) {
                    Ok(value) => value,
                    Err(error) => break 'shared_presence_tracks_membership_availability_and_book_trash Err(error.into()),
                });
                Ok::<_, DatabaseError>(
                    (match usize::try_from(count).map_err(|_| DatabaseError::message("library book count must not be negative")) {
                        Ok(value) => value,
                        Err(error) => break 'shared_presence_tracks_membership_availability_and_book_trash Err(error.into()),
                    }),
                )
            }
        })
        .unwrap(),
        0
    );
}

#[test]
fn opening_an_existing_library_installs_shared_read_views_without_republishing() {
    let holder = open_test_database();
    let db = &holder.database;
    commit_book(db, &sync_common::ROOT_DIR_ID, "7", "Existing book", "Author", "existing.epub");
    let before = db.sync_publishable_mutations().unwrap();
    db.connection.execute_batch("DROP VIEW library_book_presence;").unwrap();
    let reopened = Database::open(&holder.path).unwrap();
    reopened.initialize_library().unwrap();
    assert_eq!(
        ({
            use crate::browse::{CountsSql, HomeSql};
            use crate::DatabaseError;
            use library_model::LibraryHomeView;
            let database = &reopened;
            'opening_an_existing_library_installs_shared_read_views_without_republishing: {
                let mut count = 0_i64;
                (match database.connection.count_all_books(|row| {
                    count = row.get(0)?;
                    Ok(())
                }) {
                    Ok(value) => value,
                    Err(error) => break 'opening_an_existing_library_installs_shared_read_views_without_republishing Err(error.into()),
                });
                Ok::<_, DatabaseError>(
                    (match usize::try_from(count).map_err(|_| DatabaseError::message("library book count must not be negative")) {
                        Ok(value) => value,
                        Err(error) => break 'opening_an_existing_library_installs_shared_read_views_without_republishing Err(error.into()),
                    }),
                )
            }
        })
        .unwrap(),
        1
    );
    assert_eq!(reopened.sync_publishable_mutations().unwrap(), before);
}

#[test]
fn download_records_only_its_materialized_path_and_does_not_queue_an_upload() {
    let holder = open_test_database();
    let db = &holder.database;
    let root = sync_common::ROOT_DIR_ID;
    let copy = db.create_directory(&root, &"Copy".into()).unwrap().id;
    let hash = commit_book(db, &root, "5", "Downloaded book", "Author", "download.epub");
    db.transfer_book_placement(&hash, &root, &copy, false).unwrap();
    db.connection.execute("DELETE FROM book_transfer WHERE content_hash=?1", [hash.as_str()]).unwrap();
    let snapshot = db.transfer_snapshot(&[hash]).unwrap().placements.into_iter().find(|entry| entry.rel_path.as_str() == "/download.epub").unwrap().book.unwrap();
    let checksum = sync_common::ContentHash::new(&"b".repeat(64));
    db.complete_downloaded_book(
        crate::NativeBookCompletion {
            snapshot,
            publication: library_model::PublishedStagedBook { name: "download.epub".into(), relative_path: "/download.epub".into(), published: true, fingerprint: Some("verified".into()) },
            checksum,
            size_bytes: 2048,
        },
        true,
    )
    .unwrap();
    let recorded: (String, i64, String, bool) =
        db.connection.query_row("SELECT checksum,size_bytes,local_hash,is_downloaded FROM book_dir WHERE dir_id=?1", [root.to_string()], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap();
    assert_eq!(recorded, (checksum.to_string(), 2048, "verified".into(), true));
    let materialized: bool = db.connection.query_row("SELECT is_downloaded FROM book_dir WHERE dir_id=?1", [copy.to_string()], |row| row.get(0)).unwrap();
    assert!(!materialized, "a queued copy is not yet a physical file");
    assert!(db.is_book_downloaded(&hash).unwrap(), "the book is available from the verified source");
    assert!(db.book_upload_intent(&hash).unwrap().is_none());
    assert!(!db.native_file_work().unwrap().is_empty(), "copy materialization still has durable work");
}

#[test]
fn cover_upload_eligibility_is_shared_by_enrichment_and_transfers() {
    let holder = open_test_database();
    let db = &holder.database;
    let hash = commit_book(db, &sync_common::ROOT_DIR_ID, "6", "Cover policy", "Author", "cover.epub");
    db.connection.execute("UPDATE book_transfer SET upload_id=NULL,server_has_book=1,server_has_cover=0,cover_state='ready' WHERE content_hash=?1", [hash.as_str()]).unwrap();
    let assert_allowed = |expected| {
        assert_eq!(db.thumbnail_upload_allowed(&hash).unwrap(), expected);
        assert_eq!(
            ({
                use crate::assets::*;
                use crate::shared_sql::*;
                use crate::*;
                use library_model::{BookPlacement, BookUploadIntent, RelativeBookPath};
                use sync_common::ContentHash;
                let database = &db;
                let limit = 100;
                'assert_allowed: {
                    let mut hashes = Vec::new();
                    (match database.connection.transfer_pending_cover_uploads(
                        limit,
                        (match crate::assets::rejection_cutoff() {
                            Ok(value) => value,
                            Err(error) => break 'assert_allowed Err(error.into()),
                        }),
                        |row| {
                            hashes.push(ContentHash::new(&row.get::<_, String>(0)?));
                            Ok(())
                        },
                    ) {
                        Ok(value) => value,
                        Err(error) => break 'assert_allowed Err(error.into()),
                    });
                    Ok::<_, crate::DatabaseError>(hashes)
                }
            })
            .unwrap()
            .contains(&hash),
            expected
        );
        assert_eq!(db.transfer_snapshot(&[hash]).unwrap().assets[&hash].thumbnail_allowed, expected);
    };
    assert_allowed(false); // Parent lifecycle must first reach the server.
    db.connection.execute("DELETE FROM sync_outbox WHERE state_kind='book_lifecycle' AND state_key=?1", [hash.as_str()]).unwrap();
    assert_allowed(true); // No pending book upload is necessary.
    db.connection.execute("UPDATE book_transfer SET cover_rejected_at=unixepoch() WHERE content_hash=?1", [hash.as_str()]).unwrap();
    assert_allowed(false);
    db.connection.execute("UPDATE book_transfer SET cover_rejected_at=0,server_has_book=0 WHERE content_hash=?1", [hash.as_str()]).unwrap();
    assert_allowed(false);
    db.connection.execute("UPDATE book_transfer SET server_has_book=1,server_has_cover=1 WHERE content_hash=?1", [hash.as_str()]).unwrap();
    assert_allowed(false);
}
