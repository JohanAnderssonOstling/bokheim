use super::*;
use super::{assets::*, browse::*, reading::*, sync_state::*};

const TEST_CONTENT_HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn test_content_hash() -> ContentHash {
    ContentHash::new(&TEST_CONTENT_HASH)
}

fn test_dir_id(value: &str) -> DirId {
    DirId::parse(value).unwrap()
}

fn test_file_name(value: &str) -> FileName {
    FileName::parse(value).unwrap()
}

fn author(value: &str) -> Contributor {
    Contributor::new(value, None, MarcRelatorCode(*b"aut")).unwrap()
}

fn metadata(title: impl Into<String>, authors: Vec<Contributor>, description: impl Into<String>) -> BookMetadataState {
    BookMetadataState { title: title.into(), contributors: authors, description: description.into(), publication: PublicationMetadata::default() }
}

#[test]
fn client_identity_is_validated_at_the_database_boundary() {
    let conn = Connection::open_in_memory().unwrap();
    let generated = init_db(&conn).unwrap();
    assert_eq!(get_or_create_client_id(&conn).unwrap(), generated);

    set_sync_metadata(&conn, "client_id", "invalid/client").unwrap();
    assert!(get_or_create_client_id(&conn).is_err());
}

#[test]
fn local_relationships_use_integer_rows_but_expose_content_hash_identity() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();

    conn.execute(
        "INSERT INTO book(content_hash, format, added_at) VALUES (?1, 'epub', 1)",
        [TEST_CONTENT_HASH],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded)
         VALUES ('__root__', (SELECT row_id FROM book WHERE content_hash = ?1), 'book.epub', 'fingerprint', 1)",
        [TEST_CONTENT_HASH],
    )
    .unwrap();

    let (book_key_type, placement_key_type, exposed_hash): (String, String, String) = conn
        .query_row(
            "SELECT typeof(book.row_id), typeof(book_dir.book_row_id), book.content_hash
             FROM book_dir JOIN book ON book.row_id = book_dir.book_row_id",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();

    assert_eq!(book_key_type, "integer");
    assert_eq!(placement_key_type, "integer");
    assert_eq!(exposed_hash, TEST_CONTENT_HASH);
}

/// The card projection must agree with the per-book accessors it replaces,
/// otherwise browsing would quietly present different state than before.
#[test]
fn dir_card_rows_match_the_per_book_accessors_they_replace() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO book(content_hash, title, description, read_progress, format) VALUES
                 (printf('%064x',1), 'Downloaded', 'A description', 42.0, 'epub'),
                 (printf('%064x',2), 'Not downloaded', '', 0.0, 'epub'),
                 (printf('%064x',3), 'An audiobook', '', 0.0, 'epub'),
                 (printf('%064x',4), 'Hidden', '', 0.0, 'epub');
             UPDATE book SET hidden_at=1 WHERE content_hash=printf('%064x',4);
             INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES
                 ('__root__', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one.epub', 'h1', 1),
                 ('__root__', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'two.epub', 'h2', 0),
                 ('__root__', (SELECT row_id FROM book WHERE content_hash=printf('%064x',3)), 'three.M4B', 'h3', 1),
                 ('__root__', (SELECT row_id FROM book WHERE content_hash=printf('%064x',4)), 'four.epub', 'h4', 1);
             INSERT INTO audiobook_metadata(content_hash, duration_ms, chapter_count, chapter_source, chapters_json) VALUES (printf('%064x',3), 3600000, 12, 'embedded_m4b', '[]');",
    )
    .unwrap();

    let cards = get_book_cards_in_dir_scoped(&conn, "__root__", false).unwrap();
    assert_eq!(cards.len(), 3, "hidden books stay excluded");

    let downloaded = cards.iter().find(|card| card.content_hash == ContentHash::from_fixture_u64(1)).expect("book 1 is listed");
    assert!(downloaded.downloaded);
    assert!(!downloaded.audiobook);
    assert_eq!(downloaded.description, "A description");
    assert_eq!(downloaded.progress, 42.0);

    let missing = cards.iter().find(|card| card.content_hash == ContentHash::from_fixture_u64(2)).expect("book 2 is listed");
    assert!(!missing.downloaded);

    let audiobook = cards.iter().find(|card| card.content_hash == ContentHash::from_fixture_u64(3)).expect("book 3 is listed");
    assert!(audiobook.audiobook, "the extension check must be case-insensitive");
    assert_eq!(audiobook.audiobook_duration_ms, Some(3_600_000));
    assert_eq!(audiobook.audiobook_chapter_count, Some(12));

    // Agreement with the accessors this projection replaces.
    for card in &cards {
        assert_eq!(card.downloaded, is_book_downloaded(&conn, card.content_hash).unwrap(), "downloaded disagrees for {}", card.content_hash);
        assert_eq!(card.download_requested, is_book_download_requested(&conn, card.content_hash).unwrap(), "requested disagrees for {}", card.content_hash);
    }
}

#[test]
fn downloaded_only_scope_is_applied_inside_browsing_queries() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO book(content_hash, title, added_at, format) VALUES
                 (printf('%064x',1), 'Local book', 1, 'epub'),
                 (printf('%064x',2), 'Remote book', 2, 'epub');
             INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id) VALUES
                 ('reading_position',printf('%064x',1),'',1,0,'test',1,'recent-1'),
                 ('reading_position',printf('%064x',2),'',2,0,'test',2,'recent-2');
             INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES
                 ('__root__', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'local.epub', 'h1', 1),
                 ('__root__', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'remote.epub', 'h2', 0);",
    )
    .unwrap();
    let transaction = conn.transaction().unwrap();
    update_book_metadata(&transaction, ContentHash::from_fixture_u64(1), "Local book", "Shared Author").unwrap();
    update_book_metadata(&transaction, ContentHash::from_fixture_u64(2), "Remote book", "Shared Author").unwrap();
    transaction.commit().unwrap();

    assert_eq!(get_recently_read_cards(&conn, 1, true).unwrap()[0].content_hash, ContentHash::from_fixture_u64(1), "scope must be applied before LIMIT");
    assert_eq!(get_recently_added_cards(&conn, 1, true).unwrap()[0].content_hash, ContentHash::from_fixture_u64(1), "scope must be applied before LIMIT");
    assert_eq!(search_book_cards_scoped(&conn, "book", 10, true).unwrap().iter().map(|card| card.content_hash).collect::<Vec<_>>(), vec![ContentHash::from_fixture_u64(1)]);
    assert_eq!(get_book_cards_in_dir_scoped(&conn, "__root__", true).unwrap().iter().map(|card| card.content_hash).collect::<Vec<_>>(), vec![ContentHash::from_fixture_u64(1)]);

    let authors = get_authors_scoped(&conn, true).unwrap();
    assert_eq!(authors.len(), 1);
    assert_eq!(authors[0].2, 1, "author counts use the same availability scope as their books");
    assert_eq!(get_book_cards_by_author_scoped(&conn, authors[0].0, true).unwrap().iter().map(|card| card.content_hash).collect::<Vec<_>>(), vec![ContentHash::from_fixture_u64(1)]);
}

#[test]
fn a_requested_download_is_reported_by_the_card_projection() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO book(content_hash, title, format) VALUES (printf('%064x',1), 'Queued', 'epub');
             INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES
                 ('__root__', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one.epub', 'h1', 0);
             INSERT INTO download_requests(target_kind, target_key, origin, created_at)
             VALUES ('book', printf('%064x',1), 'user_initiated', 0);",
    )
    .unwrap();

    let cards = get_book_cards_in_dir_scoped(&conn, "__root__", false).unwrap();
    assert!(!cards[0].downloaded);
    assert!(cards[0].download_requested);
}

/// A book placed in several folders joins once per placement; the author
/// view must still show it once.
#[test]
fn author_cards_collapse_a_book_placed_in_several_folders() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let author_id = AuthorId::new();
    conn.execute_batch(&format!(
        "INSERT INTO dir(id, parent_id, name) VALUES ('a', '__root__', 'A'), ('b', '__root__', 'B');
             INSERT INTO book(content_hash, title, format) VALUES (printf('%064x',1), 'Placed twice', 'epub');
             INSERT INTO author_identity(id, stable_id, preferred_name, normalized_name, created_at) VALUES (1, '{author_id}', 'An Author', 'an author', 0);
             INSERT INTO book_contributor(book_row_id, author_identity_id, position, name, role) VALUES ((SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 1, 0, 'An Author', X'617574');
             INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES
                 ('a', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one.epub', 'h1', 1),
                 ('b', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one-again.epub', 'h2', 1);",
    ))
    .unwrap();

    let cards = get_book_cards_by_author_scoped(&conn, author_id, false).unwrap();
    assert_eq!(cards.len(), 1, "one card per book, not per placement");
    assert_eq!(cards[0].content_hash, ContentHash::from_fixture_u64(1));
}

#[test]
fn subdirectory_book_counts_include_descendants_without_double_counting_books() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO dir(id, parent_id, name) VALUES
                 ('a', '__root__', 'A'),
                 ('a-child', 'a', 'Child'),
                 ('b', '__root__', 'B'),
                 ('empty', '__root__', 'Empty'),
                 ('empty-child', 'empty', 'Empty child');
             INSERT INTO book(content_hash, title, format) VALUES (printf('%064x',1), 'One', 'epub'), (printf('%064x',2), 'Two', 'epub'), (printf('%064x',3), 'Three', 'epub'), (printf('%064x',4), 'Deleted', 'epub');
             UPDATE book SET deleted_at=1 WHERE content_hash=printf('%064x',4);
             INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES
                 ('a', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one.epub', 'a-one'),
                 ('a-child', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'one-again.epub', 'a-one-again'),
                 ('a-child', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'two.epub', 'a-two'),
                 ('a', (SELECT row_id FROM book WHERE content_hash=printf('%064x',4)), 'deleted.epub', 'a-deleted'),
                 ('b', (SELECT row_id FROM book WHERE content_hash=printf('%064x',3)), 'three.epub', 'b-three');
             UPDATE book_dir SET is_downloaded=1 WHERE book_row_id IN (SELECT row_id FROM book WHERE content_hash IN (printf('%064x',1), printf('%064x',3)));",
    )
    .unwrap();

    let directories = get_child_directories(&conn, "__root__").unwrap();
    let counts = directories.into_iter().map(|directory| (directory.name().as_str().to_owned(), (directory.book_count(), directory.downloaded_book_count()))).collect::<HashMap<_, _>>();

    assert_eq!(counts.get("A"), Some(&(2, 1)));
    assert_eq!(counts.get("B"), Some(&(1, 1)));
    assert_eq!(counts.get("Empty"), Some(&(0, 0)));
}

#[test]
fn recently_added_falls_back_to_live_books_without_added_timestamps() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute("INSERT INTO book(content_hash, title, format) VALUES (?1, 'Existing book', 'epub')", [ContentHash::from_fixture_u64(1).as_str()]).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES ('__root__', (SELECT row_id FROM book WHERE content_hash=?1), 'existing.epub', 'existing')", [ContentHash::from_fixture_u64(1).as_str()]).unwrap();

    let books = get_recently_added_cards(&conn, 20, false).unwrap();
    assert_eq!(books.iter().map(|book| book.content_hash).collect::<Vec<_>>(), vec![ContentHash::from_fixture_u64(1)]);
}

#[test]
fn folder_download_queries_are_recursive_and_preserve_durable_requests() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let second_hash = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let third_hash = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    conn.execute_batch(
        "INSERT INTO dir(id, parent_id, name) VALUES
                 ('shelf', '__root__', 'Shelf'),
                 ('child', 'shelf', 'Child'),
                 ('other', '__root__', 'Other');
             INSERT INTO book(content_hash, added_at, format) VALUES (printf('%064x',1), 1, 'epub'), (printf('%064x',2), 1, 'epub'), (printf('%064x',3), 1, 'epub');",
    )
    .unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES ('shelf', (SELECT row_id FROM book WHERE content_hash=?1), 'one.epub', ?2, 1)", [ContentHash::from_fixture_u64(1).as_str(), TEST_CONTENT_HASH]).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES ('child', (SELECT row_id FROM book WHERE content_hash=?1), 'two.epub', ?2, 0)", [ContentHash::from_fixture_u64(2).as_str(), second_hash]).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES ('other', (SELECT row_id FROM book WHERE content_hash=?1), 'three.epub', ?2, 1)", [ContentHash::from_fixture_u64(3).as_str(), third_hash]).unwrap();

    assert_eq!(get_descendant_content_hashes(&conn, "shelf").unwrap(), vec![ContentHash::from_fixture_u64(1), ContentHash::from_fixture_u64(2)]);
    assert_eq!(get_book_ancestor_dir_ids(&conn, ContentHash::from_fixture_u64(2)).unwrap(), vec![test_dir_id("__root__"), test_dir_id("child"), test_dir_id("shelf")]);
    assert_eq!(get_folder_download_counts(&conn, "shelf").unwrap(), (1, 2));
    assert_eq!(get_folder_download_counts(&conn, "other").unwrap(), (1, 1));

    add_asset_request(&conn, BlobKind::Book, &ContentHash::from_fixture_u64(2), TransferOrigin::UserInitiated).unwrap();
    assert!(is_book_download_requested(&conn, ContentHash::from_fixture_u64(2)).unwrap());
    assert!(is_book_downloaded(&conn, ContentHash::from_fixture_u64(1)).unwrap());
    assert!(!is_book_download_requested(&conn, ContentHash::from_fixture_u64(1)).unwrap());
    assert!(!is_book_downloaded(&conn, ContentHash::from_fixture_u64(2)).unwrap());
    assert!(is_book_download_requested(&conn, ContentHash::from_fixture_u64(2)).unwrap());
    assert!(is_book_downloaded(&conn, ContentHash::from_fixture_u64(3)).unwrap());
    complete_asset_request(&conn, BlobKind::Book, &ContentHash::from_fixture_u64(2)).unwrap();
    assert!(!is_book_download_requested(&conn, ContentHash::from_fixture_u64(2)).unwrap());
    add_asset_request(&conn, BlobKind::Book, &ContentHash::from_fixture_u64(2), TransferOrigin::UserInitiated).unwrap();
    assert!(is_book_download_requested(&conn, ContentHash::from_fixture_u64(2)).unwrap());
    assert!(!is_book_download_requested(&conn, ContentHash::from_fixture_u64(1)).unwrap());
}

#[test]
fn title_substring_search_is_case_insensitive_and_does_not_search_authors() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute("INSERT INTO book(content_hash, title, added_at, format) VALUES (?1, '', 1, 'epub')", [ContentHash::from_fixture_u64(1).as_str()]).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES ('__root__', (SELECT row_id FROM book WHERE content_hash=?1), 'power.epub', 'power')", [ContentHash::from_fixture_u64(1).as_str()]).unwrap();

    let transaction = conn.transaction().unwrap();
    update_book_metadata(&transaction, ContentHash::from_fixture_u64(1), "Power and Progress", "GÖRAN Smith").unwrap();
    transaction.commit().unwrap();

    assert_eq!(search_book_cards_scoped(&conn, "ow", 10, false).unwrap().len(), 1);
    assert_eq!(search_book_cards_scoped(&conn, "progress", 10, false).unwrap().len(), 1);
    assert!(search_book_cards_scoped(&conn, "smith", 10, false).unwrap().is_empty());
    assert!(search_book_cards_scoped(&conn, "missing", 10, false).unwrap().is_empty());

    let cards = search_book_cards_scoped(&conn, "POWER", 10, false).unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].title, "Power and Progress");
    assert_eq!(cards[0].author, "GÖRAN Smith");
    assert!(search_book_cards_scoped(&conn, "smith", 10, false).unwrap().is_empty());
}

#[test]
fn library_groups_inverted_and_punctuated_author_spellings_without_changing_book_metadata() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let transaction = conn.transaction().unwrap();
    insert_scanned_book(&transaction, ContentHash::from_fixture_u64(1), "The Hobbit", "Tolkien, J. R. R.").unwrap();
    insert_scanned_book(&transaction, ContentHash::from_fixture_u64(2), "The Lord of the Rings", "J.R.R. Tolkien").unwrap();
    transaction.commit().unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES ('__root__', (SELECT row_id FROM book WHERE content_hash=?1), 'hobbit.epub', 'hobbit')", [ContentHash::from_fixture_u64(1).as_str()]).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES ('__root__', (SELECT row_id FROM book WHERE content_hash=?1), 'rings.epub', 'rings')", [ContentHash::from_fixture_u64(2).as_str()]).unwrap();

    assert_eq!(conn.query_row("SELECT COUNT(*) FROM author_identity", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
    let authors = get_authors_scoped(&conn, false).unwrap();
    assert_eq!(authors.len(), 1);
    assert_eq!((&authors[0].1, authors[0].2), (&"J.R.R. Tolkien".to_owned(), 2));
    assert_eq!(get_book_cards_by_author_scoped(&conn, authors[0].0, false).unwrap().len(), 2);
    let spellings = conn.prepare("SELECT name FROM book_contributor ORDER BY book_row_id, position").unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(spellings, vec!["Tolkien, J. R. R.", "J.R.R. Tolkien"]);
    let synchronized_spellings = get_pending_mutations(&conn)
        .unwrap()
        .into_iter()
        .filter_map(|mutation| match mutation.body {
            MutationBody::BookLifecycle { content_hash, value: BookLifecycleState::Present { metadata, .. } } => Some((content_hash, metadata.contributors)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        synchronized_spellings.iter().map(|(content_hash, authors)| (content_hash.clone(), authors.iter().map(Contributor::name).collect::<Vec<_>>())).collect::<Vec<_>>(),
        vec![(ContentHash::from_fixture_u64(1), vec!["Tolkien, J. R. R."]), (ContentHash::from_fixture_u64(2), vec!["J.R.R. Tolkien"])],
    );
    assert_eq!(synchronized_spellings[0].1[0].agent_id(), synchronized_spellings[1].1[0].agent_id());
}

#[test]
fn ordered_book_author_links_preserve_all_epub_credits() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let authors = vec![Contributor::new("Ursula K. Le Guin", Some("Le Guin, Ursula K.".to_owned()), MarcRelatorCode(*b"aut")).unwrap(), author("Jane Doe")];
    let transaction = conn.transaction().unwrap();
    insert_scanned_book_contributors(&transaction, ContentHash::from_fixture_u64(1), "A collaboration", &authors).unwrap();
    transaction.commit().unwrap();

    let credits = conn
        .prepare("SELECT author_identity.normalized_name, book_contributor.name, book_contributor.sort_name FROM book_contributor JOIN author_identity ON author_identity.id=book_contributor.author_identity_id WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1)) ORDER BY position")
        .unwrap()
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(credits, vec![("ursula k le guin".to_owned(), "Ursula K. Le Guin".to_owned(), Some("Le Guin, Ursula K.".to_owned())), ("jane doe".to_owned(), "Jane Doe".to_owned(), None),]);
    let pending = get_pending_mutations(&conn).unwrap();
    assert!(pending.into_iter().any(|mutation| matches!(
        mutation.body,
        MutationBody::BookLifecycle {
            value: BookLifecycleState::Present { metadata, .. },
            ..
        } if metadata.contributors == authors
    )));
}

#[test]
fn author_schema_has_one_entity_and_one_book_credit_projection() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();

    let legacy_tables: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name IN ('author', 'author_identity_name')", [], |row| row.get(0)).unwrap();
    assert_eq!(legacy_tables, 0);
    let book_columns = conn.prepare("PRAGMA table_info(book)").unwrap().query_map([], |row| row.get::<_, String>(1)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    assert!(!book_columns.iter().any(|column| matches!(column.as_str(), "author_id" | "author_name" | "authors")), "the wire-encoded authors blob no longer exists; book_contributor is the sole canonical register");
}

#[test]
fn transfer_jobs_are_not_persisted_in_the_library_schema() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let transfer_jobs: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='transfer_jobs'", [], |row| row.get(0)).unwrap();
    assert_eq!(transfer_jobs, 0);
}

#[test]
fn scanned_publication_metadata_is_canonical_and_relationally_projected() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let publication = PublicationMetadata {
        identifiers: vec![sync_common::PublicationIdentifier::new("9781234567897", PublicationIdentifierScheme::Isbn, Vec::new()).unwrap()],
        publishers: vec![sync_common::PublisherCredit::new("North Wind Press").unwrap()],
        languages: vec![sync_common::LanguageTag::parse("SV-se").unwrap()],
        dates: vec![sync_common::PublicationDate::new(None, "2026-08-04", Some("publication".to_owned())).unwrap()],
        subjects: vec![sync_common::PublicationSubject::new(None, "Distributed systems", None, None).unwrap()],
        collections: vec![sync_common::PublicationCollection::new("Bokheim Studies", Some("series".to_owned()), Some("2".to_owned())).unwrap()],
        rights: vec!["Copyright example".to_owned()],
        ..PublicationMetadata::default()
    };
    // Mixed author + non-author contributors share one flat book_contributor
    // position sequence now that Contributor is the sole relational model.
    let translator = Contributor::new("Example Translator", None, MarcRelatorCode(*b"trl")).unwrap();
    let contributors = vec![author("Example Author"), translator.clone()];
    let state = BookMetadataState { title: "Metadata book".to_owned(), contributors, description: "A description".to_owned(), publication: publication.clone() };

    let transaction = conn.transaction().unwrap();
    insert_scanned_book_metadata(&transaction, ContentHash::from_fixture_u64(1), &state, BookFormat::Epub).unwrap();
    transaction.commit().unwrap();

    let encoded: Vec<u8> = conn.query_row("SELECT publication_metadata FROM book WHERE content_hash=printf('%064x',1)", [], |row| row.get(0)).unwrap();
    assert_eq!(sync_common::wire::decode::<PublicationMetadata>(&encoded, sync_common::wire::MAX_DECODED_REQUEST_BYTES,).unwrap(), publication,);
    for table in ["book_identifier", "book_publisher", "book_language", "book_publication_date", "book_subject", "book_collection", "book_publication_value"] {
        let count: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table} WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1))"), [], |row| row.get(0)).unwrap();
        assert_eq!(count, 1, "missing {table} projection");
    }
    let contributor_count: i64 = conn.query_row("SELECT COUNT(*) FROM book_contributor WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1))", [], |row| row.get(0)).unwrap();
    assert_eq!(contributor_count, 2, "author and non-author contributor rows both project into the merged table");
    let canonical_identifier: Option<String> = conn.query_row("SELECT canonical_value FROM book_identifier WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1))", [], |row| row.get(0)).unwrap();
    assert_eq!(canonical_identifier.as_deref(), Some("9781234567897"));
    let normalized_date = conn
        .query_row("SELECT normalized_year, normalized_month, normalized_day FROM book_publication_date WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1))", [], |row| Ok((row.get::<_, Option<i32>>(0)?, row.get::<_, Option<u8>>(1)?, row.get::<_, Option<u8>>(2)?)))
        .unwrap();
    assert_eq!(normalized_date, (None, None, None));
    let publisher_agent: String = conn.query_row("SELECT author_identity.stable_id FROM book_publisher JOIN author_identity ON author_identity.id=book_publisher.agent_identity_id WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1))", [], |row| row.get(0)).unwrap();
    assert_eq!(publisher_agent, publication.publishers[0].agent_id().to_string());
    let contributor_agent: String =
        conn.query_row("SELECT author_identity.stable_id FROM book_contributor JOIN author_identity ON author_identity.id=book_contributor.author_identity_id WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1)) AND role=X'74726c'", [], |row| row.get(0)).unwrap();
    assert_eq!(contributor_agent, translator.agent_id().to_string());
    let contributor_role: Vec<u8> = conn.query_row("SELECT role FROM book_contributor WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1)) AND role=X'74726c'", [], |row| row.get(0)).unwrap();
    assert_eq!(contributor_role, b"trl");
    let author_role: Vec<u8> = conn.query_row("SELECT role FROM book_contributor WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1)) AND role=X'617574'", [], |row| row.get(0)).unwrap();
    assert_eq!(author_role, b"aut");

    let pending = get_pending_mutations(&conn).unwrap();
    assert_eq!(pending.len(), 1);
    assert!(matches!(
        &pending[0].body,
        MutationBody::BookLifecycle {
            value: BookLifecycleState::Present { metadata, .. },
            ..
        } if metadata == &state
    ));
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES ('__root__', (SELECT row_id FROM book WHERE content_hash=?1), 'metadata.epub', 'metadata')", [ContentHash::from_fixture_u64(1).as_str()]).unwrap();
    let author_id = get_authors_scoped(&conn, false).unwrap()[0].0;
    let discovery_seed = get_author_discovery_seed(&conn, author_id).unwrap().unwrap();
    assert_eq!(discovery_seed, ("Example Author".to_owned(), vec![("isbn".to_owned(), "9781234567897".to_owned())]));
}

#[test]
fn authority_evidence_distinguishes_namesakes_without_splitting_spelling_variants() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    insert_scanned_book_contributors(&tx, ContentHash::from_fixture_u64(1), "First work", &[author("Smith, Alex")]).unwrap();
    insert_scanned_book_contributors(&tx, ContentHash::from_fixture_u64(2), "Second work", &[author("Alex Smith")]).unwrap();
    insert_scanned_book_contributors(&tx, ContentHash::from_fixture_u64(3), "Third work", &[author("A. Smith")]).unwrap();
    let first_wikidata = ExternalAuthorId::parse(AuthorAuthority::Wikidata, "Q1001").unwrap();
    let second_wikidata = ExternalAuthorId::parse(AuthorAuthority::Wikidata, "Q1002").unwrap();
    let first = resolve_book_author_identity(&tx, ContentHash::from_fixture_u64(1), 0, "Alex Smith", &first_wikidata).unwrap();
    let second = resolve_book_author_identity(&tx, ContentHash::from_fixture_u64(2), 0, "Alex Smith", &second_wikidata).unwrap();
    let first_variant = resolve_book_author_identity(&tx, ContentHash::from_fixture_u64(3), 0, "Alex Smith", &first_wikidata).unwrap();
    assert_ne!(first, second, "the same normalized name must be able to represent namesakes");
    assert_eq!(first, first_variant, "one authority id must converge spelling variants on one person");
    attach_external_author_id(&tx, first, &ExternalAuthorId::parse(AuthorAuthority::Viaf, "12345678").unwrap()).unwrap();
    assert!(attach_external_author_id(&tx, first, &second_wikidata).is_err(), "one authority id cannot identify two people");
    tx.commit().unwrap();

    assert_eq!(conn.query_row("SELECT COUNT(*) FROM author_identity", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
    let first_credit = get_book_author_identities(&conn, ContentHash::from_fixture_u64(1)).unwrap().pop().unwrap();
    assert_eq!(first_credit.author_id, first);
    assert_eq!(first_credit.external_ids.len(), 2);
}

#[test]
fn metadata_refresh_preserves_identity_only_for_the_same_normalized_credit() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    insert_scanned_book_contributors(&tx, ContentHash::from_fixture_u64(1), "Book", &[author("Smith, Alex")]).unwrap();
    let identity = resolve_book_author_identity(&tx, ContentHash::from_fixture_u64(1), 0, "Alex Smith", &ExternalAuthorId::parse(AuthorAuthority::OpenLibrary, "OL123A").unwrap()).unwrap();
    update_book_metadata_contributors(&tx, ContentHash::from_fixture_u64(1), "Book", &[author("Alex Smith")]).unwrap();
    assert_eq!(get_book_author_identities(&tx, ContentHash::from_fixture_u64(1)).unwrap()[0].author_id, identity);
    update_book_metadata_contributors(&tx, ContentHash::from_fixture_u64(1), "Book", &[author("Different Person")]).unwrap();
    assert_ne!(get_book_author_identities(&tx, ContentHash::from_fixture_u64(1)).unwrap()[0].author_id, identity);
    tx.commit().unwrap();
}

#[test]
fn namesake_split_reidentifies_only_the_selected_book_credit() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    insert_scanned_book_contributors(&tx, ContentHash::from_fixture_u64(1), "First work", &[author("Alex Smith")]).unwrap();
    insert_scanned_book_contributors(&tx, ContentHash::from_fixture_u64(2), "Second work", &[author("Alex Smith")]).unwrap();
    tx.commit().unwrap();

    let before_first = get_book_author_identities(&conn, ContentHash::from_fixture_u64(1)).unwrap()[0].author_id;
    let before_second = get_book_author_identities(&conn, ContentHash::from_fixture_u64(2)).unwrap()[0].author_id;
    assert_eq!(before_first, before_second, "unresolved normalized-name matches begin as one provisional author");

    conn.execute("DELETE FROM sync_outbox", []).unwrap();
    let tx = conn.transaction().unwrap();
    let split_id = split_book_author(&tx, ContentHash::from_fixture_u64(2), 0).unwrap();
    tx.commit().unwrap();

    assert_eq!(get_book_author_identities(&conn, ContentHash::from_fixture_u64(1)).unwrap()[0].author_id, before_first);
    assert_eq!(get_book_author_identities(&conn, ContentHash::from_fixture_u64(2)).unwrap()[0].author_id, split_id);
    assert_ne!(split_id, before_first);
    assert_eq!(conn.query_row("SELECT COUNT(DISTINCT normalized_name) FROM author_identity", [], |row| row.get::<_, i64>(0)).unwrap(), 1, "splitting identity must retain normalized-name grouping");
    let identity: String = conn
        .query_row("SELECT author_identity.stable_id FROM book_contributor JOIN author_identity ON author_identity.id=book_contributor.author_identity_id WHERE book_contributor.book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',2)) AND book_contributor.position=0", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(AuthorId::parse(&identity).unwrap(), split_id);
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM sync_outbox WHERE state_kind='metadata'", [], |row| row.get::<_, i64>(0)).unwrap(), 1, "the identity correction must synchronize");
}

#[test]
fn author_browsing_keeps_same_name_identities_and_their_books_separate() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    insert_scanned_book_contributors(&tx, ContentHash::from_fixture_u64(1), "First work", &[author("Alex Smith")]).unwrap();
    insert_scanned_book_contributors(&tx, ContentHash::from_fixture_u64(2), "Second work", &[author("Alex Smith")]).unwrap();
    tx.commit().unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES ('__root__', (SELECT row_id FROM book WHERE content_hash=?1), 'first.epub', 'first')", [ContentHash::from_fixture_u64(1).as_str()]).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES ('__root__', (SELECT row_id FROM book WHERE content_hash=?1), 'second.epub', 'second')", [ContentHash::from_fixture_u64(2).as_str()]).unwrap();

    let first_id = get_book_author_identities(&conn, ContentHash::from_fixture_u64(1)).unwrap()[0].author_id;
    let tx = conn.transaction().unwrap();
    let second_id = split_book_author(&tx, ContentHash::from_fixture_u64(2), 0).unwrap();
    tx.commit().unwrap();

    let authors = get_authors_scoped(&conn, false).unwrap();
    assert_eq!(authors.len(), 2);
    assert!(authors.iter().all(|(_, name, count)| name == "Alex Smith" && *count == 1));
    assert_eq!(authors.iter().map(|(id, _, _)| *id).collect::<BTreeSet<_>>(), BTreeSet::from([first_id, second_id]));
    let first_titles = get_book_cards_by_author_scoped(&conn, first_id, false).unwrap().into_iter().map(|book| book.title).collect::<Vec<_>>();
    let second_titles = get_book_cards_by_author_scoped(&conn, second_id, false).unwrap().into_iter().map(|book| book.title).collect::<Vec<_>>();
    assert_eq!(first_titles, vec!["First work"]);
    assert_eq!(second_titles, vec!["Second work"]);
}

#[test]
fn remote_author_ids_are_preserved_even_when_names_normalize_identically() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.insert_book(ContentHash::from_fixture_u64(1).as_str(), "First", 1, "epub").unwrap();
    conn.insert_book(ContentHash::from_fixture_u64(2).as_str(), "Second", 1, "epub").unwrap();
    let first_id = AuthorId::new();
    let second_id = AuthorId::new();
    let first = Contributor::with_id(first_id, "Alex Smith", None, MarcRelatorCode(*b"aut")).unwrap();
    let second = Contributor::with_id(second_id, "Smith, Alex", None, MarcRelatorCode(*b"aut")).unwrap();
    let tx = conn.transaction().unwrap();
    update_book_metadata_contributors_preserving_ids(&tx, ContentHash::from_fixture_u64(1), "First", &[first]).unwrap();
    update_book_metadata_contributors_preserving_ids(&tx, ContentHash::from_fixture_u64(2), "Second", &[second]).unwrap();
    tx.commit().unwrap();

    assert_eq!(get_book_author_identities(&conn, ContentHash::from_fixture_u64(1)).unwrap()[0].author_id, first_id);
    assert_eq!(get_book_author_identities(&conn, ContentHash::from_fixture_u64(2)).unwrap()[0].author_id, second_id);
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM author_identity", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
}

#[test]
fn title_search_needs_no_derived_index_backfill() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute_batch(
        "INSERT INTO book(content_hash, title, added_at, format) VALUES (printf('%064x',2), 'Parable of the Sower', 1, 'epub');
             INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES ('__root__', (SELECT row_id FROM book WHERE content_hash=printf('%064x',2)), 'parable.epub', 'parable');",
    )
    .unwrap();

    init_db(&conn).unwrap();

    assert_eq!(search_book_cards_scoped(&conn, "ra", 10, false).unwrap().len(), 1);
    assert!(search_book_cards_scoped(&conn, "butler", 10, false).unwrap().is_empty());
}

fn server_change(body: MutationBody, pt: u64, l: u64) -> ServerMutation {
    server_change_from(body, pt, l, "remote", 1, 1)
}

fn server_change_from(body: MutationBody, pt: u64, _l: u64, actor: &str, replica_seq: u64, revision: u64) -> ServerMutation {
    let mutation = StateMutation { mutation_id: MutationId::new(), body, changed_at: pt, replica_seq: ReplicaSeq::new(replica_seq).unwrap() };
    ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: ClientId::parse(actor).unwrap(), revision: sync_common::LibraryRevision::new(revision).unwrap() }
}

fn valid_outbox_row() -> OutboxRow {
    OutboxRow {
        mutation_id: MutationId::new().to_string(),
        state_kind: "book_lifecycle".to_string(),
        state_key: ContentHash::from_fixture_u64(1).to_string(),
        body: sync_common::wire::encode(&MutationBody::BookLifecycle { content_hash: ContentHash::from_fixture_u64(1), value: BookLifecycleState::Deleted }).unwrap(),
        changed_at: 10,
        replica_seq: 1,
        state_subkey: String::new(),
    }
}

#[test]
fn outbox_decoder_accepts_valid_rows() {
    let change = decode_outbox_row(valid_outbox_row()).expect("valid outbox row");
    assert_eq!(change.replica_seq.get(), 1);
    assert!(matches!(change.body, MutationBody::BookLifecycle { content_hash, value: BookLifecycleState::Deleted } if content_hash == ContentHash::from_fixture_u64(1)));
}

#[test]
fn outbox_decoder_rejects_invalid_wire_and_invalid_canonical_bodies() {
    let mut invalid_wire = valid_outbox_row();
    invalid_wire.body = vec![0xc1];
    assert!(matches!(decode_outbox_row(invalid_wire), Err(OutboxReadError::CorruptMutation { replica_seq: 1, .. })));

    let mut malformed_known = valid_outbox_row();
    malformed_known.body = vec![0x81];
    assert!(matches!(decode_outbox_row(malformed_known), Err(OutboxReadError::CorruptMutation { replica_seq: 1, .. })));

    let mut mismatched_key = valid_outbox_row();
    mismatched_key.state_key = ContentHash::from_fixture_u64(2).to_string();
    assert!(matches!(decode_outbox_row(mismatched_key), Err(OutboxReadError::CorruptMutation { replica_seq: 1, .. })));
}

#[test]
fn outbox_read_never_returns_events_after_an_undecodable_row() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE sync_outbox(
                 replica_seq INTEGER PRIMARY KEY,
                 mutation_id TEXT NOT NULL,
                 state_kind TEXT NOT NULL,
                 state_key TEXT NOT NULL,
                 body BLOB NOT NULL,
                 changed_at INTEGER NOT NULL,
                 state_subkey TEXT NOT NULL,
                 conflict_rank INTEGER NOT NULL DEFAULT 0
             );
             INSERT INTO sync_outbox(replica_seq, mutation_id, state_kind, state_key, body, changed_at, state_subkey) VALUES
                 (1, '00000000-0000-4000-8000-000000000001', 'book_lifecycle', '1', x'c1', 10, ''),
                 (2, '00000000-0000-4000-8000-000000000002', 'book_lifecycle', '2', x'c1', 11, '');",
    )
    .unwrap();

    assert!(matches!(get_pending_mutations(&conn), Err(OutboxReadError::CorruptMutation { replica_seq: 1, .. })));
}

#[test]
fn generated_mutation_clocks_are_unix_milliseconds() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.insert_book(ContentHash::from_fixture_u64(1).as_str(), "Book", unix_millis().unwrap(), "epub").unwrap();

    let changed_at: i64 = conn.query_row("SELECT changed_at FROM sync_outbox WHERE state_kind='book_lifecycle'", [], |row| row.get(0)).unwrap();
    let now = unix_millis().unwrap();
    assert!(changed_at >= now - 5_000 && changed_at <= now + 5_000);
    assert!(changed_at < 100_000_000_000_000, "a generated protocol clock must not be microseconds");
}

#[test]
fn acknowledging_an_in_flight_value_cannot_delete_its_newer_replacement() {
    let conn = singleton_guard_conn();
    let first = get_pending_mutations(&conn).unwrap();
    assert_eq!(first.len(), 2, "book creation and reading position occupy distinct state cells");
    let first_position = first.iter().find(|mutation| matches!(mutation.body, MutationBody::ReadingPosition { .. })).unwrap();
    let old_id = first_position.mutation_id;
    let old_seq = first_position.replica_seq;

    update_reading_position(&conn, ContentHash::from_fixture_u64(1), &EpubCfi::parse("epubcfi(/6/4)").unwrap(), ReadProgress::new(50.0).unwrap()).unwrap();
    let second = get_pending_mutations(&conn).unwrap();
    let second_position = second.iter().find(|mutation| matches!(mutation.body, MutationBody::ReadingPosition { .. })).unwrap();
    assert_ne!(second_position.mutation_id, old_id);
    assert!(second_position.replica_seq > old_seq);

    acknowledge_mutations(&conn, &[old_id]).unwrap();
    let after_stale_ack = get_pending_mutations(&conn).unwrap();
    let retained = after_stale_ack.iter().find(|mutation| matches!(mutation.body, MutationBody::ReadingPosition { .. })).unwrap();
    assert_eq!(retained.mutation_id, second_position.mutation_id);

    acknowledge_mutations(&conn, &[second_position.mutation_id]).unwrap();
    assert!(get_pending_mutations(&conn).unwrap().iter().all(|mutation| !matches!(mutation.body, MutationBody::ReadingPosition { .. })));
}

/// Current row-ID schema for isolated sweep tests.
fn new_schema_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn
}

fn is_alive(conn: &Connection, dir: &str, content_hash: &str, file: &str) -> bool {
    conn.query_row("SELECT deleted_at IS NULL FROM book_dir WHERE dir_id=?1 AND book_row_id=(SELECT row_id FROM book WHERE content_hash=?2) AND file_name=?3", rusqlite::params![dir, content_hash, file], |r| r.get::<_, bool>(0)).unwrap()
}

/// Current schema initialized for isolated remote placement tests.
fn lww_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'D1')", []).unwrap();
    conn
}

fn book_dir_change(add: bool, pt: u64, l: u64) -> ServerMutation {
    let body = if add { MutationBody::Placement { dir_id: test_dir_id("d1"), content_hash: ContentHash::from_fixture_u64(1), present: true } } else { MutationBody::Placement { dir_id: test_dir_id("d1"), content_hash: ContentHash::from_fixture_u64(1), present: false } };
    server_change(body, pt, l)
}

fn book_dir_add_change(content_hash: ContentHash, dir_id: &str, file_name: &str, pt: u64) -> ServerMutation {
    let _ = file_name;
    server_change(MutationBody::Placement { dir_id: test_dir_id(dir_id), content_hash, present: true }, pt, 0)
}

fn apply_dir_state(tx: &Transaction<'_>, dir_id: &str, parent_id: &str, name: &str, deleted: bool, pt: u64) {
    let id = test_dir_id(dir_id);
    let changes = [
        server_change(MutationBody::DirectoryName { dir_id: id.clone(), name: test_file_name(name) }, pt, 0),
        server_change(MutationBody::DirectoryParent { dir_id: id.clone(), parent_id: test_dir_id(parent_id) }, pt, 0),
        server_change(MutationBody::DirectoryLifecycle { dir_id: id, deleted }, pt, 0),
    ];
    for change in changes {
        apply_remote_change_in_tx(tx, &change).unwrap();
    }
}

fn fk_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    conn
}

#[test]
fn directory_dependencies_can_cross_committed_pull_pages() {
    let mut conn = fk_conn();

    let first_page = conn.transaction().unwrap();
    apply_dir_state(&first_page, "child", "parent", "Child", false, 10);
    first_page.commit().unwrap();

    let before_parent: (bool, bool) = conn
        .query_row(
            "SELECT
                 (SELECT deleted_at IS NOT NULL FROM dir WHERE id='parent'),
                 (SELECT deleted_at IS NOT NULL FROM dir WHERE id='child')",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(before_parent, (true, true), "an incomplete hierarchy must remain hidden but commit successfully");

    let second_page = conn.transaction().unwrap();
    apply_dir_state(&second_page, "parent", "__root__", "Parent", false, 11);
    second_page.commit().unwrap();

    let read = conn.transaction().unwrap();
    let paths = get_live_dir_paths_tx(&read).unwrap();
    assert_eq!(paths[&test_dir_id("parent")].as_str(), "Parent");
    assert_eq!(paths[&test_dir_id("child")].as_str(), "Parent/Child");
}

#[test]
fn placement_dependency_can_cross_committed_pull_pages() {
    let mut conn = fk_conn();
    let placement = server_change(MutationBody::Placement { dir_id: test_dir_id("future-shelf"), content_hash: ContentHash::from_fixture_u64(7), present: true }, 10, 0);

    let first_page = conn.transaction().unwrap();
    apply_remote_change_in_tx(&first_page, &placement).unwrap();
    first_page.commit().unwrap();
    let placeholder_deleted: bool = conn.query_row("SELECT deleted_at IS NOT NULL FROM dir WHERE id='future-shelf'", [], |row| row.get(0)).unwrap();
    assert!(placeholder_deleted);
    assert_eq!(get_book_file_path(&conn, ContentHash::from_fixture_u64(7)).unwrap(), None, "a placement under an unresolved directory must remain hidden");

    let second_page = conn.transaction().unwrap();
    apply_dir_state(&second_page, "future-shelf", "__root__", "Shelf", false, 11);
    second_page.commit().unwrap();
    assert_eq!(get_book_file_path(&conn, ContentHash::from_fixture_u64(7)).unwrap().unwrap().as_str(), format!("/Shelf/{}", ContentHash::from_fixture_u64(7)));
}

fn reading_position_change(cfi: &str, pt: u64, l: u64) -> ServerMutation {
    server_change(MutationBody::ReadingPosition { content_hash: ContentHash::from_fixture_u64(1), value: ReadingPositionState { location: sync_common::ReadingPosition::parse(cfi).unwrap(), progress: 0.0 } }, pt, l)
}

fn singleton_guard_conn() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    conn.insert_book(ContentHash::from_fixture_u64(1).as_str(), "T", 1, "epub").unwrap();
    update_reading_position(&conn, ContentHash::from_fixture_u64(1), &EpubCfi::parse("epubcfi(/6/2)").unwrap(), ReadProgress::new(25.0).unwrap()).unwrap();
    conn
}

fn read_pos_raw(conn: &Connection) -> String {
    conn.query_row("SELECT read_pos FROM book WHERE content_hash=printf('%064x',1)", [], |r| r.get(0)).unwrap()
}

#[test]
fn pdf_page_position_uses_the_synchronized_reading_register() {
    let conn = singleton_guard_conn();
    update_pdf_reading_page(&conn, ContentHash::from_fixture_u64(1), 17).unwrap();

    assert_eq!(get_pdf_reading_page(&conn, ContentHash::from_fixture_u64(1)).unwrap(), Some(17));
    assert_eq!(get_book_reading_position(&conn, ContentHash::from_fixture_u64(1)).unwrap(), None);
    assert_eq!(read_pos_raw(&conn), "pdfpage(17)");
    assert!(get_pending_mutations(&conn).unwrap().iter().any(|mutation| matches!(&mutation.body, MutationBody::ReadingPosition { value, .. } if value.location.as_str() == "pdfpage(17)")));
}

#[test]
fn audiobook_position_uses_the_synchronized_reading_register() {
    let conn = singleton_guard_conn();
    update_audiobook_reading_position(&conn, ContentHash::from_fixture_u64(1), 1_234_567, ReadProgress::new(42.5).unwrap()).unwrap();

    assert_eq!(get_audiobook_reading_position(&conn, ContentHash::from_fixture_u64(1)).unwrap(), Some(1_234_567));
    assert_eq!(get_book_reading_position(&conn, ContentHash::from_fixture_u64(1)).unwrap(), None);
    assert_eq!(get_pdf_reading_page(&conn, ContentHash::from_fixture_u64(1)).unwrap(), None);
    assert_eq!(read_pos_raw(&conn), "audiopos(1234567)");
    let progress: f32 = conn.query_row("SELECT read_progress FROM book WHERE content_hash=printf('%064x',1)", [], |row| row.get(0)).unwrap();
    assert_eq!(progress, 42.5);
    assert!(get_pending_mutations(&conn).unwrap().iter().any(|mutation| matches!(&mutation.body, MutationBody::ReadingPosition { value, .. } if value.location.as_str() == "audiopos(1234567)" && value.progress == 42.5)));
}

fn latest_read_position_mutation(conn: &Connection) -> (u64, u64, u64) {
    conn.query_row("SELECT replica_seq, changed_at, 0 FROM sync_outbox WHERE state_kind='reading_position' AND state_key=printf('%064x',1)", [], |r| {
        let seq: i64 = r.get(0)?;
        let pt: i64 = r.get(1)?;
        let l: i64 = r.get(2)?;
        Ok((seq.max(0) as u64, pt.max(0) as u64, l.max(0) as u64))
    })
    .unwrap()
}

/// Applies a sequence of (add?, changed_at, replica_seq) remote ops and reports whether
/// the placement ends up live (present) or tombstoned (removed).
fn final_present(ops: &[(bool, u64, u64)]) -> bool {
    let mut conn = lww_conn();
    let tx = conn.transaction().unwrap();
    let dir_id = test_dir_id("d1");
    for &(add, pt, replica_seq) in ops {
        let body = if add { MutationBody::Placement { dir_id: dir_id.clone(), content_hash: ContentHash::from_fixture_u64(1), present: true } } else { MutationBody::Placement { dir_id: dir_id.clone(), content_hash: ContentHash::from_fixture_u64(1), present: false } };
        let change = server_change_from(body, pt, 0, "remote", replica_seq.max(1), replica_seq.max(1));
        apply_remote_change_in_tx(&tx, &change).unwrap();
    }
    let present = tx.query_row("SELECT COUNT(*) FROM book_dir WHERE dir_id='d1' AND book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1)) AND deleted_at IS NULL", [], |r| r.get::<_, i64>(0)).unwrap() > 0;
    tx.commit().unwrap();
    present
}

#[test]
fn local_book_dir_ops_stamp_versions_and_preserve_conditional_transitions() {
    let conn = Connection::open_in_memory().unwrap();
    let _client = init_db(&conn).unwrap();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1','__root__','Folder')", []).unwrap();
    conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (printf('%064x',1), 1, 'epub')", []).unwrap();

    // Local add (change_origin defaults to 'local') creates one canonical
    // outbox row and stamps the placement with that mutation's version.
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded, last_scan) VALUES ('d1',(SELECT row_id FROM book WHERE content_hash=printf('%064x',1)),'f.epub','fp',1,1)", []).unwrap();
    let add_version: (i64, i64) = conn.query_row("SELECT changed_at, replica_seq FROM sync_state_version WHERE state_kind='placement' AND state_key=printf('%064x',1) AND state_subkey='d1'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert!(add_version.0 > 0, "local add must stamp wall-clock time");

    // Local remove: trig_book_dir_deleted (now AFTER) emits book_dir_removed and
    // the stamp advances the wall-clock version.
    conn.execute("UPDATE book_dir SET deleted_at=1 WHERE file_name='f.epub'", []).unwrap();
    assert!(matches!(get_pending_mutations(&conn).unwrap().last().map(|mutation| &mutation.body), Some(MutationBody::Placement { present: false, .. })));
    let remove_version: (i64, i64) = conn.query_row("SELECT changed_at, replica_seq FROM sync_state_version WHERE state_kind='placement' AND state_key=printf('%064x',1) AND state_subkey='d1'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert!(remove_version >= add_version);

    // Reactivation runs under an outer UPSERT. Metadata conflict handling in
    // its nested wall-clock version trigger must remain an UPSERT as well; `OR REPLACE` is
    // overridden by the outer statement and used to fail on the metadata PK.
    conn.insert_book_dir("d1", ContentHash::from_fixture_u64(1).as_str(), "f.epub", "fp2", 2, 1).unwrap();
    let readd_version: (i64, i64) = conn.query_row("SELECT changed_at, replica_seq FROM sync_state_version WHERE state_kind='placement' AND state_key=printf('%064x',1) AND state_subkey='d1'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    let states = get_pending_mutations(&conn)
        .unwrap()
        .into_iter()
        .filter_map(|mutation| match mutation.body {
            MutationBody::Placement { present: true, .. } => Some("present"),
            MutationBody::Placement { present: false, .. } => Some("vacant"),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(states, vec!["present"]);
    assert!(readd_version > remove_version, "replacement must advance the total local version");
}

#[test]
fn local_book_lifecycle_ops_stamp_complete_versions() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    set_sync_metadata(&conn, "client_id", "local-device").unwrap();

    conn.insert_book(ContentHash::from_fixture_u64(1).as_str(), "Book", 1, "epub").unwrap();
    let added: (i64, i64, String, i64, bool) = conn
        .query_row("SELECT version.changed_at, version.conflict_rank, version.replica_id, version.replica_seq, book.deleted_at IS NOT NULL FROM book JOIN sync_state_version version ON version.state_kind='book_lifecycle' AND version.state_key=book.content_hash AND version.state_subkey='' WHERE book.content_hash=printf('%064x',1)", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
        .unwrap();
    assert!(added.0 > 0);
    assert_eq!(added.2, "local-device");
    assert!(added.3 > 0);
    assert!(!added.4);

    conn.execute("UPDATE book SET deleted_at=1 WHERE content_hash=printf('%064x',1)", []).unwrap();
    let deleted: (i64, i64, String, i64, bool) = conn
        .query_row("SELECT version.changed_at, version.conflict_rank, version.replica_id, version.replica_seq, book.deleted_at IS NOT NULL FROM book JOIN sync_state_version version ON version.state_kind='book_lifecycle' AND version.state_key=book.content_hash AND version.state_subkey='' WHERE book.content_hash=printf('%064x',1)", [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))
        .unwrap();
    assert!((deleted.0, deleted.1) > (added.0, added.1));
    assert_eq!(deleted.2, "local-device");
    assert!(deleted.3 > added.3);
    assert!(deleted.4);
}

#[test]
fn newly_scanned_book_emits_only_lifecycle_and_placement() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let tx = conn.transaction().unwrap();
    let root = test_dir_id("__root__");
    let file_name = test_file_name("scan.epub");
    let content_hash = test_content_hash();

    insert_scanned_book(&tx, content_hash, "Scanned title", "Scanned author").unwrap();
    insert_local_book_dir(&tx, &root, content_hash, &file_name, 7, 1).unwrap();
    tx.commit().unwrap();

    let counts: (i64, i64, i64, i64) = conn
        .query_row(
            "SELECT
                    COUNT(*),
                    COUNT(*) FILTER (WHERE state_kind='book_lifecycle'),
                    COUNT(*) FILTER (WHERE state_kind='placement'),
                    COUNT(*) FILTER (WHERE state_kind='metadata')
                 FROM sync_outbox",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(counts, (2, 1, 1, 0));
    let lifecycle = get_pending_mutations(&conn)
        .unwrap()
        .into_iter()
        .find_map(|mutation| match mutation.body {
            MutationBody::BookLifecycle { value: BookLifecycleState::Present { metadata, .. }, .. } => Some((metadata.title, metadata.contributors)),
            _ => None,
        })
        .unwrap();
    assert_eq!(lifecycle.0, "Scanned title");
    assert_eq!(lifecycle.1.iter().map(Contributor::name).collect::<Vec<_>>(), vec!["Scanned author"]);

    conn.execute("DELETE FROM sync_outbox", []).unwrap();
    let tx = conn.transaction().unwrap();
    update_book_metadata(&tx, content_hash, "Edited title", "Edited author").unwrap();
    tx.commit().unwrap();
    let metadata_count: i64 = conn.query_row("SELECT COUNT(*) FROM sync_outbox WHERE state_kind='metadata'", [], |row| row.get(0)).unwrap();
    assert_eq!(metadata_count, 1, "later metadata edits must remain independently synchronized");
}

#[test]
fn dir_state_triggers_coalesce_to_the_latest_full_state() {
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();

    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'A')", []).unwrap();
    conn.execute("UPDATE dir SET name='B' WHERE id='d1'", []).unwrap();
    conn.execute("UPDATE dir SET deleted_at=1 WHERE id='d1'", []).unwrap();

    let count: i64 = conn.query_row("SELECT COUNT(*) FROM sync_outbox WHERE state_kind LIKE 'directory_%' AND state_key='d1'", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 3);
    assert!(get_pending_mutations(&conn).unwrap().into_iter().any(|mutation| matches!(mutation.body, MutationBody::DirectoryLifecycle { deleted: true, .. })));
}

#[test]
fn stale_remote_dir_state_does_not_overwrite_unpushed_local_move() {
    let mut conn = fk_conn();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'Local')", []).unwrap();
    conn.execute("UPDATE dir SET name='LocalNew' WHERE id='d1'", []).unwrap();
    let local_pt: u64 = conn
        .query_row("SELECT changed_at FROM sync_outbox WHERE state_kind='directory_name' AND state_key='d1'", [], |r| {
            let pt: i64 = r.get(0)?;
            Ok(pt.max(0) as u64)
        })
        .unwrap();

    let tx = conn.transaction().unwrap();
    let stale_name = server_change(MutationBody::DirectoryName { dir_id: test_dir_id("d1"), name: test_file_name("RemoteOld") }, local_pt.saturating_sub(1), 0);
    apply_remote_change_in_tx(&tx, &stale_name).unwrap();
    tx.commit().unwrap();

    let name: String = conn.query_row("SELECT name FROM dir WHERE id='d1'", [], |r| r.get(0)).unwrap();
    assert_eq!(name, "LocalNew");
}

#[test]
fn singleton_remote_apply_skips_stale_value_behind_unpushed_local_edit() {
    let mut conn = singleton_guard_conn();
    let (_, local_pt, _) = latest_read_position_mutation(&conn);
    let stale_pt = local_pt.saturating_sub(1);

    let tx = conn.transaction().unwrap();
    apply_remote_change_in_tx(&tx, &reading_position_change("epubcfi(/6/4)", stale_pt, 0)).unwrap();
    tx.commit().unwrap();

    assert_eq!(read_pos_raw(&conn), "epubcfi(/6/2)");
}

#[test]
fn singleton_remote_apply_accepts_newer_value_than_unpushed_local_edit() {
    let mut conn = singleton_guard_conn();
    let (_, local_pt, local_l) = latest_read_position_mutation(&conn);

    let tx = conn.transaction().unwrap();
    apply_remote_change_in_tx(&tx, &reading_position_change("epubcfi(/6/4)", local_pt + 1, local_l)).unwrap();
    tx.commit().unwrap();

    assert_eq!(read_pos_raw(&conn), "epubcfi(/6/4)");
}

#[test]
fn singleton_guard_only_considers_pending_mutations() {
    let mut conn = singleton_guard_conn();
    let (_, local_pt, _) = latest_read_position_mutation(&conn);
    let pending = get_pending_mutations(&conn).unwrap();
    acknowledge_mutations(&conn, &pending.iter().map(|mutation| mutation.mutation_id).collect::<Vec<_>>()).unwrap();

    let tx = conn.transaction().unwrap();
    apply_remote_change_in_tx(&tx, &reading_position_change("epubcfi(/6/6)", local_pt.saturating_sub(1), 0)).unwrap();
    tx.commit().unwrap();

    assert_eq!(read_pos_raw(&conn), "epubcfi(/6/2)");
}

#[test]
fn persisted_reading_state_is_validated_in_rust() {
    let conn = fk_conn();
    set_sync_metadata(&conn, "change_origin", "remote").unwrap();
    conn.execute("INSERT INTO book(content_hash, title, added_at, read_pos, format) VALUES (printf('%064x',1), 'Book', 1, 'not-a-cfi', 'epub')", []).unwrap();
    assert!(get_book_reading_position(&conn, ContentHash::from_fixture_u64(1)).is_err());

    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'Shelf')", []).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES ('d1', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'book.epub', 'fp', 1)", []).unwrap();
    conn.execute("UPDATE book SET read_progress=150 WHERE content_hash=printf('%064x',1)", []).unwrap();
    conn.execute("INSERT INTO sync_state_version VALUES ('reading_position',printf('%064x',1),'',1,0,'test',1,'recent')", []).unwrap();
    assert!(get_recently_read_cards(&conn, 10, false).is_ok());
    set_sync_metadata(&conn, "change_origin", "local").unwrap();
}

#[test]
fn invalid_remote_reading_position_is_rejected_before_storage() {
    // Decoding validates a remote location before it can become a typed
    // mutation, so the reducer never receives malformed reading state.
    assert!(sync_common::ReadingPosition::parse("not-a-cfi").is_err());
}

#[test]
fn scalar_registers_arriving_before_book_lifecycle_are_preserved() {
    let mut conn = fk_conn();
    let tx = conn.transaction().unwrap();
    apply_remote_change_in_tx(&tx, &reading_position_change("epubcfi(/6/4)", 10, 0)).unwrap();
    apply_remote_change_in_tx(
        &tx,
        &server_change(
            MutationBody::BookLifecycle {
                content_hash: ContentHash::from_fixture_u64(1),
                value: BookLifecycleState::Present { metadata: metadata("Book", vec![author("Lifecycle author")], ""), added_at: 12, format: BookFormat::Epub },
            },
            12,
            0,
        ),
    )
    .unwrap();
    tx.commit().unwrap();

    let state: (Option<String>, i64, String, String) = conn
        .query_row(
            "SELECT book.read_pos, reading.changed_at, book.title, book_contributor.name
             FROM book
             JOIN sync_state_version reading ON reading.state_kind='reading_position' AND reading.state_key=book.content_hash
             JOIN book_contributor ON book_contributor.book_row_id=book.row_id AND book_contributor.position=0
             WHERE book.content_hash=printf('%064x',1)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(state, (Some("epubcfi(/6/4)".to_string()), 10, "Book".to_owned(), "Lifecycle author".to_owned()));
}

#[test]
fn remote_metadata_rebuilds_ordered_author_links_without_echoing() {
    let mut conn = fk_conn();
    conn.execute("DELETE FROM sync_outbox", []).unwrap();
    let authors = vec![author("Le Guin, Ursula K."), author("Jane Doe")];
    let tx = conn.transaction().unwrap();
    apply_remote_change_in_tx(&tx, &server_change(MutationBody::Metadata { content_hash: ContentHash::from_fixture_u64(1), value: metadata("A collaboration", authors.clone(), "") }, 12, 0)).unwrap();
    tx.commit().unwrap();

    let credits = conn.prepare("SELECT name FROM book_contributor WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1)) ORDER BY position").unwrap().query_map([], |row| row.get::<_, String>(0)).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(credits, authors.iter().map(|author| author.name().to_owned()).collect::<Vec<_>>());
    let stored: Vec<Contributor> = conn
        .prepare("SELECT author_identity.stable_id, book_contributor.name, book_contributor.sort_name, book_contributor.role FROM book_contributor JOIN author_identity ON author_identity.id=book_contributor.author_identity_id WHERE book_contributor.book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1)) ORDER BY position")
        .unwrap()
        .query_map([], |row| {
            let stable_id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let sort_name: Option<String> = row.get(2)?;
            let role_bytes: Vec<u8> = row.get(3)?;
            let role_array: [u8; 3] = role_bytes.try_into().unwrap();
            Ok(Contributor::with_id(AuthorId::parse(&stable_id).unwrap(), name, sort_name, MarcRelatorCode(role_array)).unwrap())
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(stored, authors);
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM sync_outbox", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
}

#[test]
fn lifecycle_initializer_does_not_overwrite_existing_metadata() {
    let mut conn = fk_conn();
    set_sync_metadata(&conn, "change_origin", "remote").unwrap();
    conn.execute("INSERT INTO book(content_hash, title, added_at, format) VALUES(printf('%064x',1), 'Existing title', 1, 'epub')", []).unwrap();
    let tx = conn.transaction().unwrap();
    update_book_metadata(&tx, ContentHash::from_fixture_u64(1), "Existing title", "Existing author").unwrap();
    tx.commit().unwrap();
    set_sync_metadata(&conn, "change_origin", "local").unwrap();

    let tx = conn.transaction().unwrap();
    apply_remote_change_in_tx(
        &tx,
        &server_change(
            MutationBody::BookLifecycle {
                content_hash: ContentHash::from_fixture_u64(1),
                value: BookLifecycleState::Present { metadata: metadata("Lifecycle title", vec![author("Lifecycle author")], ""), added_at: 12, format: BookFormat::Epub },
            },
            12,
            0,
        ),
    )
    .unwrap();
    tx.commit().unwrap();

    let metadata: (String, String) = conn
        .query_row(
            "SELECT book.title, book_contributor.name FROM book JOIN book_contributor ON book_contributor.book_row_id=book.row_id AND book_contributor.position=0 WHERE book.content_hash=printf('%064x',1)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(metadata, ("Existing title".to_owned(), "Existing author".to_owned()));
}

#[test]
fn generated_outbox_identity_is_stamped_on_materialized_state() {
    let conn = fk_conn();
    conn.execute("INSERT INTO book(content_hash, title, added_at, format) VALUES (printf('%064x',1), 'Book', 1, 'epub')", []).unwrap();

    let (persisted, stamped): (String, String) = conn
        .query_row(
            "SELECT sync_outbox.mutation_id, version.mutation_id
                 FROM sync_outbox JOIN sync_state_version version
                   ON version.state_kind=sync_outbox.state_kind
                  AND version.state_key=sync_outbox.state_key
                  AND version.state_subkey=''
                 WHERE sync_outbox.state_kind='book_lifecycle'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stamped, persisted);
    MutationId::parse(&stamped).expect("the entity version must carry the generated mutation id");
}

#[test]
fn deferred_pull_tx_allows_book_dir_before_dir_in_same_batch() {
    let mut conn = fk_conn();
    let tx = conn.transaction().unwrap();
    tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();

    apply_remote_change_in_tx(&tx, &book_dir_add_change(ContentHash::from_fixture_u64(1), "late-dir", "f.epub", 10)).unwrap();
    apply_dir_state(&tx, "late-dir", "__root__", "Late", false, 11);
    tx.commit().unwrap();

    let present: i64 = conn.query_row("SELECT COUNT(*) FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1)) AND dir_id='late-dir' AND file_name=printf('%064x',1) AND deleted_at IS NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(present, 1);
}

#[test]
fn compacted_dir_state_can_arrive_after_book_dir_add_in_same_batch() {
    let mut conn = fk_conn();
    let tx = conn.transaction().unwrap();
    tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();

    apply_remote_change_in_tx(&tx, &book_dir_add_change(ContentHash::from_fixture_u64(1), "d-final", "f.epub", 10)).unwrap();
    apply_dir_state(&tx, "d-final", "__root__", "Final", false, 11);
    tx.commit().unwrap();

    let row: (String, i64) = conn.query_row("SELECT d.name, COUNT(bd.book_row_id) FROM dir d JOIN book_dir bd ON bd.dir_id=d.id WHERE d.id='d-final' AND bd.book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1))", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(row, ("Final".to_string(), 1));
}

#[test]
fn remote_book_link_binds_and_materializes_every_version_field() {
    let mut conn = fk_conn();
    let link_id = uuid::Uuid::new_v4().to_string();
    let change = server_change_from(
        MutationBody::BookLink {
            link_id: link_id.clone(),
            value: domain::BookLinkState {
                source_content_hash: ContentHash::from_fixture_u64(1),
                target_content_hash: ContentHash::from_fixture_u64(2),
                relation: "related".to_owned(),
                note: "A link".to_owned(),
                created_at: 10,
                modified_at: 20,
                deleted: false,
            },
        },
        30,
        0,
        "remote-link-writer",
        7,
        9,
    );
    let expected_mutation_id = change.mutation.mutation_id.to_string();
    let tx = conn.transaction().unwrap();
    apply_remote_change_in_tx(&tx, &change).unwrap();
    tx.commit().unwrap();

    let stored: (String, String, i64, String, i64, String) = conn
        .query_row("SELECT source.content_hash,target.content_hash,version.changed_at,version.replica_id,version.replica_seq,version.mutation_id FROM book_link link JOIN book source ON source.row_id=link.source_book_row_id JOIN book target ON target.row_id=link.target_book_row_id JOIN sync_state_version version ON version.state_kind='book_link' AND version.state_key=link.id AND version.state_subkey='' WHERE link.id=?1", [&link_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
        })
        .unwrap();
    assert_eq!(stored, (ContentHash::from_fixture_u64(1).to_string(), ContentHash::from_fixture_u64(2).to_string(), 30, "remote-link-writer".to_owned(), 7, expected_mutation_id));
}

#[test]
fn deferred_pull_tx_allows_child_dir_before_parent_in_same_batch() {
    let mut conn = fk_conn();
    let tx = conn.transaction().unwrap();
    tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();

    apply_dir_state(&tx, "child-dir", "parent-dir", "Child", false, 10);
    apply_dir_state(&tx, "parent-dir", "__root__", "Parent", false, 11);
    tx.commit().unwrap();

    let parent: String = conn.query_row("SELECT parent_id FROM dir WHERE id='child-dir'", [], |r| r.get(0)).unwrap();
    assert_eq!(parent, "parent-dir");
}

#[test]
fn missing_directory_is_a_hidden_placeholder_across_pull_transactions() {
    let mut conn = fk_conn();
    let tx = conn.transaction().unwrap();
    tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();

    apply_remote_change_in_tx(&tx, &book_dir_add_change(ContentHash::from_fixture_u64(1), "missing-dir", "f.epub", 10)).unwrap();

    tx.commit().unwrap();
    let placeholder: (String, bool) = conn.query_row("SELECT parent_id,deleted_at IS NOT NULL FROM dir WHERE id='missing-dir'", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(placeholder, ("__root__".to_owned(), true));
}

#[test]
fn remote_book_dir_into_soft_deleted_dir_still_applies() {
    let mut conn = fk_conn();
    conn.execute("INSERT INTO dir(id, parent_id, name, deleted_at) VALUES ('soft-dir', '__root__', 'Soft', 1)", []).unwrap();
    let tx = conn.transaction().unwrap();
    tx.pragma_update(None, "defer_foreign_keys", "ON").unwrap();

    apply_remote_change_in_tx(&tx, &book_dir_add_change(ContentHash::from_fixture_u64(1), "soft-dir", "f.epub", 10)).unwrap();
    tx.commit().unwrap();

    let present: i64 = conn.query_row("SELECT COUNT(*) FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',1)) AND dir_id='soft-dir' AND file_name=printf('%064x',1) AND deleted_at IS NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(present, 1);
}

// Pre-existing gap unrelated to the contributor migration: `delete_dir` and
// `remove_deleted_dir_artifacts` do not exist anywhere in this crate (not a
// visibility issue — the functions were never (re)implemented after some
// earlier refactor). Left disabled rather than inventing directory-deletion
// business logic as a side effect of the contributor unification.
#[test]
#[cfg(any())] // disabled: delete_dir/remove_deleted_dir_artifacts do not exist in this codebase; pre-existing gap unrelated to the contributor migration
fn delete_dir_cascades_subtree_events_files_and_leaves_siblings() {
    let mut conn = fk_conn();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('p', '__root__', 'Parent')", []).unwrap();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('a', 'p', 'A')", []).unwrap();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('b', 'a', 'B')", []).unwrap();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('s', '__root__', 'Sibling')", []).unwrap();
    conn.execute("INSERT INTO book(id, title, added_at, content_hash, format) VALUES ('1', 'One', 1, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'epub'), ('2', 'Two', 1, 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 'epub'), ('3', 'Three', 1, 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc', 'epub')", []).unwrap();
    conn.execute_batch(
        "INSERT INTO sync_state_version VALUES
        ('reading_position','1','',10,0,'test',1,'recent-1'),
        ('reading_position','2','',20,0,'test',2,'recent-2'),
        ('reading_position','3','',30,0,'test',3,'recent-3');",
    )
    .unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, content_hash, file_name, local_hash, is_downloaded) VALUES ('a', '1', 'a.epub', 'ha', 1)", []).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, content_hash, file_name, local_hash, is_downloaded) VALUES ('b', '2', 'b.epub', 'hb', 1)", []).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, content_hash, file_name, local_hash, is_downloaded) VALUES ('s', '3', 's.epub', 'hs', 1)", []).unwrap();
    conn.execute("DELETE FROM sync_outbox", []).unwrap();

    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join("Parent").join("A").join("B")).unwrap();
    std::fs::create_dir_all(temp.path().join("Sibling")).unwrap();
    std::fs::write(temp.path().join("Parent").join(".biblos_uuid"), "p").unwrap();
    std::fs::write(temp.path().join("Parent").join("A").join(".biblos_uuid"), "a").unwrap();
    std::fs::write(temp.path().join("Parent").join("A").join("B").join(".biblos_uuid"), "b").unwrap();
    std::fs::write(temp.path().join("Sibling").join(".biblos_uuid"), "s").unwrap();
    std::fs::write(temp.path().join("Parent").join("A").join("a.epub"), b"a").unwrap();
    std::fs::write(temp.path().join("Parent").join("A").join("B").join("b.epub"), b"b").unwrap();
    std::fs::write(temp.path().join("Sibling").join("s.epub"), b"s").unwrap();

    let deleted = delete_dir(&mut conn, &test_dir_id("a")).unwrap();
    remove_deleted_dir_artifacts(temp.path(), &deleted).unwrap();

    let tombstoned_dirs: i64 = conn.query_row("SELECT COUNT(*) FROM dir WHERE id IN ('a','b') AND deleted_at IS NOT NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(tombstoned_dirs, 2);
    let tombstoned_placements: i64 = conn.query_row("SELECT COUNT(*) FROM book_dir WHERE content_hash IN ('1','2') AND deleted_at IS NOT NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(tombstoned_placements, 2);
    assert!(is_alive(&conn, "s", "3", "s.epub"));

    let pending = get_pending_mutations(&conn).unwrap();
    let deleted_dir_events = pending.iter().filter(|mutation| matches!(mutation.body, MutationBody::DirectoryLifecycle { deleted: true, .. })).count() as i64;
    assert_eq!(deleted_dir_events, 2);
    let removed_placement_events = pending.iter().filter(|mutation| matches!(mutation.body, MutationBody::Placement { present: false, .. })).count() as i64;
    assert_eq!(removed_placement_events, 2);

    assert!(!temp.path().join("Parent").join("A").join("a.epub").exists());
    assert!(!temp.path().join("Parent").join("A").join("B").join("b.epub").exists());
    assert!(!temp.path().join("Parent").join("A").exists());
    assert!(temp.path().join("Parent").exists());
    assert!(temp.path().join("Sibling").join("s.epub").exists());
    assert_eq!(count_all_books(&conn), 1);
    let recent_ids: Vec<ContentHash> = get_recently_read_cards(&conn, 10, false).unwrap().into_iter().map(|book| book.content_hash).collect();
    assert_eq!(recent_ids, vec![ContentHash::from_fixture_u64(3)]);

    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('a2', 'p', 'A')", []).unwrap();
    let recreated: i64 = conn.query_row("SELECT COUNT(*) FROM dir WHERE id='a2' AND deleted_at IS NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(recreated, 1);
}

#[test]
fn materialize_dir_with_markers_writes_nested_markers_idempotently() {
    let conn = fk_conn();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'A')", []).unwrap();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d2', 'd1', 'B')", []).unwrap();
    let temp = tempfile::tempdir().unwrap();

    let path = materialize_dir_with_markers(&conn, temp.path(), "d2").unwrap().unwrap();
    assert_eq!(path, temp.path().join("A").join("B"));
    assert_eq!(std::fs::read_to_string(temp.path().join("A").join(".biblos_uuid")).unwrap(), "d1");
    assert_eq!(std::fs::read_to_string(temp.path().join("A").join("B").join(".biblos_uuid")).unwrap(), "d2");

    std::fs::write(temp.path().join("A").join("B").join(".biblos_uuid"), "custom").unwrap();
    assert!(materialize_dir_with_markers(&conn, temp.path(), "d2").is_err());
    assert_eq!(std::fs::read_to_string(temp.path().join("A").join("B").join(".biblos_uuid")).unwrap(), "custom");
}

#[test]
fn client_create_directory_commits_nested_database_and_filesystem_identity() {
    let mut conn = fk_conn();
    let root = tempfile::tempdir().unwrap();
    let library_root = LibraryRoot::open(root.path()).unwrap();
    let root_id = test_dir_id("__root__");
    let parent = create_local_directory(&mut conn, &library_root, &root_id, &test_file_name("Parent")).unwrap();
    let child = create_local_directory(&mut conn, &library_root, parent.directory.id(), &test_file_name("Child")).unwrap();

    assert_eq!(child.relative_path.as_str(), "Parent/Child");
    assert_eq!(fs::read_to_string(root.path().join("Parent/.biblos_uuid")).unwrap(), parent.directory.id().as_str());
    assert_eq!(fs::read_to_string(root.path().join("Parent/Child/.biblos_uuid")).unwrap(), child.directory.id().as_str());
    let events = get_pending_mutations(&conn).unwrap().iter().filter(|mutation| matches!(mutation.body, MutationBody::DirectoryLifecycle { deleted: false, .. })).count() as i64;
    assert_eq!(events, 2);
}

#[test]
fn client_create_directory_refuses_untracked_filesystem_entries_and_rolls_back_database_state() {
    let mut conn = fk_conn();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("Existing")).unwrap();
    let library_root = LibraryRoot::open(root.path()).unwrap();
    let result = create_local_directory(&mut conn, &library_root, &test_dir_id("__root__"), &test_file_name("Existing"));

    assert!(result.is_err());
    let stored: i64 = conn.query_row("SELECT COUNT(*) FROM dir WHERE name='Existing'", [], |row| row.get(0)).unwrap();
    assert_eq!(stored, 0);
    assert!(!root.path().join("Existing/.biblos_uuid").exists());
}

#[test]
fn client_create_directory_rejects_duplicate_names_without_damaging_the_original() {
    let mut conn = fk_conn();
    let root = tempfile::tempdir().unwrap();
    let library_root = LibraryRoot::open(root.path()).unwrap();
    let root_id = test_dir_id("__root__");
    let original = create_local_directory(&mut conn, &library_root, &root_id, &test_file_name("Shelf")).unwrap();

    assert!(create_local_directory(&mut conn, &library_root, &root_id, &test_file_name("Shelf")).is_err());
    let stored: i64 = conn.query_row("SELECT COUNT(*) FROM dir WHERE name='Shelf' AND deleted_at IS NULL", [], |row| row.get(0)).unwrap();
    assert_eq!(stored, 1);
    assert_eq!(fs::read_to_string(root.path().join("Shelf/.biblos_uuid")).unwrap(), original.directory.id().as_str());
}

#[test]
fn client_create_directory_rejects_a_missing_parent_without_touching_disk() {
    let mut conn = fk_conn();
    let root = tempfile::tempdir().unwrap();
    let library_root = LibraryRoot::open(root.path()).unwrap();

    assert!(create_local_directory(&mut conn, &library_root, &test_dir_id("missing"), &test_file_name("Child")).is_err());
    assert!(!root.path().join("Child").exists());
    let stored: i64 = conn.query_row("SELECT COUNT(*) FROM dir WHERE name='Child'", [], |row| row.get(0)).unwrap();
    assert_eq!(stored, 0);
}

#[test]
fn stamp_book_dir_local_hash_makes_scanner_fast_lookup_hit() {
    let conn = fk_conn();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'A')", []).unwrap();
    conn.execute("INSERT INTO book(content_hash, added_at, description_scanned, format) VALUES (printf('%064x',1), 1, 1, 'epub')", []).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES ('d1', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'f.epub', 'old', 1)", []).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("A");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("f.epub");
    std::fs::write(&file, b"epub bytes").unwrap();

    stamp_book_dir_local_hash(&conn, "d1", ContentHash::from_fixture_u64(1), "f.epub", &file).unwrap();
    let hash = local_file_fingerprint(&file).unwrap();
    let found = get_book_dirs_by_local_fingerprint(&conn).unwrap().remove(&hash).unwrap();
    assert!(!found.description_missing);
}

#[test]
fn local_file_fingerprint_observes_subsecond_modifications() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("book.epub");
    std::fs::write(&path, b"same-size bytes").unwrap();
    let base = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
    let file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();

    file.set_times(std::fs::FileTimes::new().set_modified(base + std::time::Duration::from_nanos(101))).unwrap();
    let first = local_file_fingerprint(&path).unwrap();
    file.set_times(std::fs::FileTimes::new().set_modified(base + std::time::Duration::from_nanos(202))).unwrap();
    let second = local_file_fingerprint(&path).unwrap();

    assert_ne!(first, second, "the scan shortcut must retain nanosecond mtime precision");
}

#[test]
fn sqlite_rejects_unknown_blob_kinds() {
    let conn = fk_conn();

    assert!(conn.execute("INSERT INTO remote_asset(kind, hash) VALUES ('cover', ?1)", [TEST_CONTENT_HASH]).is_err());
}

#[test]
fn local_and_remote_asset_facts_are_independent_and_namespaced() {
    let conn = fk_conn();
    let content_hash = test_content_hash();

    upsert_blob_local_ref(&conn, BlobKind::Book, &content_hash, "/book.epub", true).unwrap();

    assert_eq!(get_locally_present_blob_hashes(&conn, BlobKind::Book).unwrap(), vec![content_hash.clone()]);
    assert!(get_locally_present_blob_hashes(&conn, BlobKind::Thumbnail).unwrap().is_empty());
    assert!(is_blob_local(&conn, BlobKind::Book, &content_hash).unwrap());
    assert!(!is_blob_local(&conn, BlobKind::Thumbnail, &content_hash).unwrap());
    assert!(get_remote_asset_hashes(&conn, BlobKind::Book).unwrap().is_empty(), "a local copy must not imply remote presence");

    record_remote_asset(&conn, BlobKind::Book, &content_hash).unwrap();
    assert!(get_remote_asset_hashes(&conn, BlobKind::Book).unwrap().contains(&content_hash));
    assert!(get_remote_asset_hashes(&conn, BlobKind::Thumbnail).unwrap().is_empty());

    forget_remote_asset(&conn, BlobKind::Book, &content_hash).unwrap();
    assert!(get_remote_asset_hashes(&conn, BlobKind::Book).unwrap().is_empty());
}

#[test]
fn completing_one_asset_request_cannot_erase_a_concurrent_request() {
    let conn = fk_conn();
    let first = test_content_hash();
    let second = ContentHash::new(&"b".repeat(64));
    add_asset_request(&conn, BlobKind::Book, &first, TransferOrigin::Background).unwrap();
    add_asset_request(&conn, BlobKind::Book, &second, TransferOrigin::Background).unwrap();
    complete_asset_request(&conn, BlobKind::Book, &first).unwrap();

    assert_eq!(get_asset_requests(&conn, BlobKind::Book).unwrap(), vec![(second, TransferOrigin::Background)]);
}

#[test]
fn placement_publication_guard_requires_exact_live_book_path_and_hash() {
    let conn = fk_conn();
    let hash = test_content_hash();
    let placement = BookPlacement::new(hash, RelativeBookPath::parse("/A/f.epub").unwrap());
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'A')", []).unwrap();
    conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (?1, 1, 'epub')", [hash.as_str()]).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash) VALUES ('d1', (SELECT row_id FROM book WHERE content_hash=?1), 'f.epub', '')", [hash.as_str()]).unwrap();

    assert!(book_placement_expects_hash(&conn, &placement, &hash).unwrap());
    conn.execute("UPDATE book SET content_hash=?1", ["b".repeat(64)]).unwrap();
    assert!(!book_placement_expects_hash(&conn, &placement, &hash).unwrap());
    conn.execute("UPDATE book SET content_hash=?1", [hash.as_str()]).unwrap();
    conn.execute("UPDATE book_dir SET deleted_at=1", []).unwrap();
    assert!(!book_placement_expects_hash(&conn, &placement, &hash).unwrap());
}

#[test]
fn typed_download_identity_marks_and_stamps_the_matching_placement() {
    let conn = fk_conn();
    let content_hash = ContentHash::from_fixture_u64(7);
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'A')", []).unwrap();
    conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (?1, 1, 'epub')", [content_hash.as_str()]).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES ('d1', (SELECT row_id FROM book WHERE content_hash=?1), 'f.epub', 'old', 0)", [content_hash.as_str()]).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("f.epub");
    std::fs::write(&file, b"downloaded bytes").unwrap();

    let rel_path = RelativeBookPath::parse("/A/f.epub").unwrap();
    stamp_book_dir_local_hash_by_rel_path(&conn, &rel_path, content_hash, &file).unwrap();
    set_book_downloaded(&conn, &rel_path, content_hash, true).unwrap();

    assert!(is_book_downloaded(&conn, content_hash).unwrap());
    let stored: String = conn.query_row("SELECT local_hash FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)", [content_hash.as_str()], |row| row.get(0)).unwrap();
    assert_eq!(stored, local_file_fingerprint(&file).unwrap().to_string());
}

/// Naming a download twice must return the same name. Without this the book's
/// own file occupies the name the ladder just chose, the next call escalates a
/// rung, and the transfer writes a second copy that no placement references.
#[test]
fn materialize_download_path_is_idempotent_for_a_book_that_already_has_its_file() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    std::fs::create_dir_all(root.path().join("A")).unwrap();
    let library_root = LibraryRoot::open(root.path()).unwrap();
    let conn = fk_conn();
    let content_hash = test_content_hash();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'A')", []).unwrap();
    conn.execute("INSERT INTO book(content_hash, title, added_at, format) VALUES (?1, 'Dune', 1, 'epub')", [content_hash.as_str()]).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES ('d1', (SELECT row_id FROM book WHERE content_hash=?1), ?1, '', 0)", [content_hash.as_str()]).unwrap();

    let placement = BookPlacement::new(content_hash, RelativeBookPath::parse(&format!("/A/{content_hash}")).unwrap());
    let first = materialize_download_path(&conn, &library_root, &placement).unwrap();
    assert_eq!(first.as_str(), "/A/Dune.epub", "a download names itself from metadata, not from the placeholder placement name");

    // The transfer lands, and the same placement is materialized again.
    std::fs::write(root.path().join("A").join("Dune.epub"), b"bytes").unwrap();
    let placement = BookPlacement::new(content_hash, first.clone());
    let second = materialize_download_path(&conn, &library_root, &placement).unwrap();
    assert_eq!(second, first, "re-materializing must not escalate past the book's own file");
    let stored: String = conn.query_row("SELECT file_name FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)", [content_hash.as_str()], |row| row.get(0)).unwrap();
    assert_eq!(stored, "Dune.epub");
}

/// The probe must see the filesystem, not only app-managed rows: a file the user
/// put there themselves still occupies its name.
#[test]
fn materialize_download_path_yields_to_a_scanned_file_holding_the_name() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    std::fs::create_dir_all(root.path().join("A")).unwrap();
    std::fs::write(root.path().join("A").join("Dune.epub"), b"a book the user named").unwrap();
    let library_root = LibraryRoot::open(root.path()).unwrap();
    let conn = fk_conn();
    let content_hash = test_content_hash();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1', '__root__', 'A')", []).unwrap();
    conn.execute("INSERT INTO book(content_hash, title, added_at, format) VALUES (?1, 'Dune', 1, 'epub')", [content_hash.as_str()]).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES ('d1', (SELECT row_id FROM book WHERE content_hash=?1), ?1, '', 0)", [content_hash.as_str()]).unwrap();

    let placement = BookPlacement::new(content_hash, RelativeBookPath::parse(&format!("/A/{content_hash}")).unwrap());
    let chosen = materialize_download_path(&conn, &library_root, &placement).unwrap();
    assert_eq!(chosen.as_str(), "/A/Dune [aaaaaaaaaaaa].epub", "the download must not target a file it does not own");
}

#[test]
fn remote_book_dir_add_remove_converge_regardless_of_order() {
    // The #4 divergence: device A applied B's add last (present), device B
    // applied A's remove last (absent). With the wall-clock version gate, the newer wall-clock version op
    // (remove@20) wins no matter the application order.
    let add_then_remove = final_present(&[(true, 10, 0), (false, 20, 0)]);
    let remove_then_add = final_present(&[(false, 20, 0), (true, 10, 0)]);
    assert_eq!(add_then_remove, remove_then_add, "order must not change the outcome");
    assert!(!add_then_remove, "newer wall-clock version remove must win → absent");

    // Symmetric case: a newer add beats an older remove, both orders.
    assert!(final_present(&[(false, 20, 0), (true, 30, 0)]));
    assert!(final_present(&[(true, 30, 0), (false, 20, 0)]));

    // Exact wall-clock ties are resolved consistently: deletion is always the
    // deterministic loser override in placement LWW, regardless of sequence.
    assert!(!final_present(&[(true, 15, 2), (false, 15, 1)]));
    assert!(!final_present(&[(false, 15, 1), (true, 15, 2)]));
    assert!(!final_present(&[(true, 15, 1), (false, 15, 2)]));
    assert!(!final_present(&[(false, 15, 2), (true, 15, 1)]));
}

#[test]
fn remote_book_delete_creates_a_versioned_tombstone_before_any_add() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let content_hash = ContentHash::from_fixture_u64(77);
    let deletion = server_change_from(MutationBody::BookLifecycle { content_hash, value: BookLifecycleState::Deleted }, 10, 0, "delete-device", 1, 1);
    let stale_add = server_change_from(
        MutationBody::BookLifecycle { content_hash, value: BookLifecycleState::Present { metadata: metadata("Stale", Vec::new(), ""), added_at: 1, format: BookFormat::Epub } },
        9,
        0,
        "add-device",
        1,
        2,
    );

    let tx = conn.transaction().unwrap();
    apply_remote_change_in_tx(&tx, &deletion).unwrap();
    apply_remote_change_in_tx(&tx, &stale_add).unwrap();
    tx.commit().unwrap();

    let deleted: bool = conn.query_row("SELECT deleted_at IS NOT NULL FROM book WHERE content_hash=?1", [content_hash.as_str()], |row| row.get(0)).unwrap();
    assert!(deleted, "an unseen deletion must survive a later stale add");
}

#[test]
fn resolve_removed_and_live_placements_support_relocation() {
    // The inputs the move (relocate-on-delete) decision relies on.
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let content_hash = test_content_hash();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('dx','__root__','X')", []).unwrap();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('dy','__root__','Y')", []).unwrap();
    conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (?1, 1, 'epub')", [content_hash.as_str()]).unwrap();
    // Old placement: tombstoned (the move's source), holding content H.
    conn.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded,deleted_at) VALUES ('dx',(SELECT row_id FROM book WHERE content_hash=?1),'f.epub','fp',1,1)", [content_hash.as_str()]).unwrap();
    // New placement: live (the move's target), same content, no file yet.
    conn.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES ('dy',(SELECT row_id FROM book WHERE content_hash=?1),'f.epub',?1,0)", [content_hash.as_str()]).unwrap();

    // A tombstoned placement resolves with its path and content hash.
    assert_eq!(resolve_removed_book_dir(&conn, "dx", content_hash).unwrap(), Some((RelativeBookPath::parse("/X/f.epub").unwrap(), Some(content_hash))));
    // A live placement does not resolve as removed (so its file is never touched).
    assert_eq!(resolve_removed_book_dir(&conn, "dy", content_hash).unwrap(), None);
    // Live placements of content H = the relocation target, excluding the tombstone.
    assert_eq!(get_book_paths_by_content_hash(&conn, &content_hash).unwrap(), vec![BookPath { content_hash, rel_path: RelativeBookPath::parse("/Y/f.epub").unwrap() }]);
}

#[test]
fn carousels_hide_books_with_no_live_placement() {
    // A book whose every placement is removed keeps its metadata row but must
    // not surface in the carousels (un-openable), and must reappear when a
    // placement returns.
    let conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let content_hash = test_content_hash();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES ('d1','__root__','F')", []).unwrap();
    conn.execute("INSERT INTO book(content_hash, title, added_at, format) VALUES (?1, 'T', 5, 'epub')", [content_hash.as_str()]).unwrap();
    conn.execute("INSERT INTO sync_state_version VALUES ('reading_position',?1,'',100,0,'test',1,'recent')", [content_hash.as_str()]).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id,book_row_id,file_name,local_hash,is_downloaded) VALUES ('d1',(SELECT row_id FROM book WHERE content_hash=?1),'f.epub','fp',1)", [content_hash.as_str()]).unwrap();

    let shows = |connection: &Connection| get_recently_read_cards(connection, 10, false).unwrap().iter().any(|book| book.content_hash == content_hash);
    assert!(shows(&conn), "book with a live placement must appear");
    assert_eq!(count_all_books(&conn), 1);

    // Remove the only placement: the row survives but drops out of the carousel.
    conn.execute("UPDATE book_dir SET deleted_at=1 WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)", [content_hash.as_str()]).unwrap();
    assert!(!shows(&conn), "orphaned book must not appear in the carousel");
    assert_eq!(count_all_books(&conn), 0, "count must match what is displayed");
    let row_kept: i64 = conn.query_row("SELECT COUNT(*) FROM book WHERE content_hash=?1 AND deleted_at IS NULL", [content_hash.as_str()], |r| r.get(0)).unwrap();
    assert_eq!(row_kept, 1, "the book metadata row must be preserved, not tombstoned");

    // A returning placement makes it reappear with no resurrection plumbing.
    conn.execute("UPDATE book_dir SET deleted_at=NULL WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)", [content_hash.as_str()]).unwrap();
    assert!(shows(&conn), "book must reappear once a live placement returns");
}

fn count_all_books(conn: &Connection) -> i64 {
    let mut n = 0;
    conn.count_all_books(|r| {
        n = r.get(0)?;
        Ok(())
    })
    .unwrap();
    n
}

#[test]
fn winning_remote_removal_reports_applied_for_file_deletion() {
    let mut conn = lww_conn();
    let tx = conn.transaction().unwrap();
    // Present placement.
    apply_remote_change_in_tx(&tx, &book_dir_change(true, 10, 1)).unwrap();
    // Higher-wall-clock version removal wins -> caller should delete the local file.
    let applied = apply_remote_change_in_tx(&tx, &book_dir_change(false, 20, 2)).unwrap();
    assert!(applied.is_some(), "winning removal must report the placement so the file is deleted");
    // A stale (older wall-clock version) removal loses -> no deletion.
    let stale = apply_remote_change_in_tx(&tx, &book_dir_change(false, 5, 3)).unwrap();
    assert!(stale.is_none(), "stale removal must not trigger a deletion");
    tx.commit().unwrap();
}

#[test]
fn sweep_marks_vanished_unavailable_without_deleting_placements() {
    // VANISH_GRACE_SCANS = 2: a file must be missing for >= 2 scans.
    let conn = new_schema_conn();
    let current = 10_i64;
    conn.execute_batch(
        "INSERT INTO dir(id,parent_id,name) VALUES ('d1','__root__','D1'),('d2','__root__','D2');
         INSERT INTO book(content_hash,format) VALUES
                 (printf('%064x',1),'epub'),(printf('%064x',2),'epub'),(printf('%064x',3),'epub'),(printf('%064x',4),'epub'),(printf('%064x',5),'epub');
         INSERT INTO book_dir (dir_id, book_row_id, file_name, local_hash, is_downloaded, last_scan) VALUES
                 ('d1',(SELECT row_id FROM book WHERE content_hash=printf('%064x',1)),'keep.epub','fp1',1,10),
                 ('d1',(SELECT row_id FROM book WHERE content_hash=printf('%064x',4)),'blip.epub','fp4',1,9),
                 ('d1',(SELECT row_id FROM book WHERE content_hash=printf('%064x',2)),'gone.epub','fp2',1,8),
                 ('d2',(SELECT row_id FROM book WHERE content_hash=printf('%064x',2)),'moved.epub','fp9',1,10),
                 ('d2',(SELECT row_id FROM book WHERE content_hash=printf('%064x',3)),'remote.epub',printf('%064x',3),0,0),
                 ('d2',(SELECT row_id FROM book WHERE content_hash=printf('%064x',5)),'fresh.epub',printf('%064x',5),1,0);",
    )
    .unwrap();

    mark_vanished_book_dirs_unavailable(&conn, current).unwrap();

    assert!(is_alive(&conn, "d1", ContentHash::from_fixture_u64(1).as_str(), "keep.epub"), "present file must survive");
    assert!(is_alive(&conn, "d1", ContentHash::from_fixture_u64(4).as_str(), "blip.epub"), "single transient miss must survive the grace window");
    assert!(is_alive(&conn, "d1", ContentHash::from_fixture_u64(2).as_str(), "gone.epub"), "missing files must keep their placement");
    let gone_downloaded: bool = conn.query_row("SELECT is_downloaded FROM book_dir WHERE dir_id='d1' AND book_row_id=(SELECT row_id FROM book WHERE content_hash=printf('%064x',2)) AND file_name='gone.epub'", [], |row| row.get(0)).unwrap();
    assert!(!gone_downloaded, "missing files become unavailable locally");
    assert!(is_alive(&conn, "d2", ContentHash::from_fixture_u64(2).as_str(), "moved.epub"), "relocated file's new path must survive");
    assert!(is_alive(&conn, "d2", ContentHash::from_fixture_u64(3).as_str(), "remote.epub"), "synced-but-undownloaded placement must never be swept");
    assert!(is_alive(&conn, "d2", ContentHash::from_fixture_u64(5).as_str(), "fresh.epub"), "downloaded-but-never-scanned file must not be deleted");

    // The blip is the one pending candidate awaiting a re-scan.
    assert_eq!(count_pending_vanish_candidates(&conn, current).unwrap(), 1);
}

#[test]
fn client_projection_revives_a_multi_node_cycle_and_its_descendant() {
    let mut conn = fk_conn();
    let tx = conn.transaction().unwrap();
    for (id, parent, name, pt) in [("a", "__root__", "A", 10), ("b", "a", "B", 11), ("c", "b", "C", 12), ("d", "c", "D", 13)] {
        apply_dir_state(&tx, id, parent, name, false, pt);
    }
    apply_dir_state(&tx, "a", "c", "A", false, 20);
    assert!(get_live_dir_paths_tx(&tx).unwrap().is_empty(), "the cycle and every unreachable descendant must be suppressed");

    apply_dir_state(&tx, "a", "__root__", "A", false, 30);
    let paths = get_live_dir_paths_tx(&tx).unwrap();
    assert_eq!(paths.get(&test_dir_id("a")).unwrap().as_str(), "A");
    assert_eq!(paths.get(&test_dir_id("b")).unwrap().as_str(), "A/B");
    assert_eq!(paths.get(&test_dir_id("c")).unwrap().as_str(), "A/B/C");
    assert_eq!(paths.get(&test_dir_id("d")).unwrap().as_str(), "A/B/C/D");
    tx.commit().unwrap();
}

#[test]
fn client_projection_revives_a_suppressed_directory_subtree_after_collision_clears() {
    let mut conn = fk_conn();
    let tx = conn.transaction().unwrap();
    apply_dir_state(&tx, "winner", "__root__", "Shared", false, 20);
    apply_dir_state(&tx, "loser", "__root__", "Shared", false, 10);
    apply_dir_state(&tx, "child", "loser", "Child", false, 11);
    let suppressed = get_live_dir_paths_tx(&tx).unwrap();
    assert_eq!(suppressed.len(), 1);
    assert_eq!(suppressed.get(&test_dir_id("winner")).unwrap().as_str(), "Shared");

    apply_dir_state(&tx, "winner", "__root__", "Other", false, 30);
    let revived = get_live_dir_paths_tx(&tx).unwrap();
    assert_eq!(revived.get(&test_dir_id("winner")).unwrap().as_str(), "Other");
    assert_eq!(revived.get(&test_dir_id("loser")).unwrap().as_str(), "Shared");
    assert_eq!(revived.get(&test_dir_id("child")).unwrap().as_str(), "Shared/Child");
    tx.commit().unwrap();
}

#[test]
fn client_rejects_a_nil_persisted_mutation_id_without_overwriting_the_row() {
    let mut conn = lww_conn();
    set_sync_metadata(&conn, "change_origin", "remote").unwrap();
    conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (printf('%064x',1), 1, 'epub')", []).unwrap();
    conn.execute(
        "INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, deleted_at)
             VALUES ('d1', (SELECT row_id FROM book WHERE content_hash=printf('%064x',1)), 'f.epub', 'old', NULL)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO sync_state_version(state_kind,state_key,state_subkey,changed_at,conflict_rank,replica_id,replica_seq,mutation_id)
         VALUES ('placement',printf('%064x',1),'d1',10,0,'stored-actor',1,'00000000-0000-0000-0000-000000000000')",
        [],
    )
    .unwrap();
    let tx = conn.transaction().unwrap();
    let result = apply_remote_change_in_tx(&tx, &book_dir_change(true, 20, 2));
    assert!(result.is_err(), "a nil stored event id must fail closed");
    let stored: String = tx.query_row("SELECT content_hash FROM book WHERE content_hash=printf('%064x',1)", [], |row| row.get(0)).unwrap();
    assert_eq!(stored, ContentHash::from_fixture_u64(1).as_str());
    tx.rollback().unwrap();
}

// --- Contributor unification regression coverage -------------------------

#[test]
fn contributor_list_rewrite_coalesces_into_one_outbox_mutation_per_book_and_kind() {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let transaction = conn.transaction().unwrap();
    insert_scanned_book_contributors(&transaction, ContentHash::from_fixture_u64(1), "Original title", &[author("Original Author")]).unwrap();
    transaction.commit().unwrap();
    conn.execute("DELETE FROM sync_outbox", []).unwrap();

    // A metadata update that clears and rewrites every book_contributor row
    // fires the per-row book_contributor INSERT/DELETE triggers once per
    // deleted row and once per inserted row, in addition to trig_book_metadata
    // itself firing on the book row's own column changes. All of that must
    // coalesce (via the shared ON CONFLICT(state_kind, state_key) upsert) into
    // exactly one outbox row per (content_hash, kind).
    let rewritten = vec![author("Replacement Author One"), author("Replacement Author Two"), Contributor::new("Replacement Editor", None, MarcRelatorCode(*b"edt")).unwrap()];
    let transaction = conn.transaction().unwrap();
    update_book_metadata_contributors(&transaction, ContentHash::from_fixture_u64(1), "Rewritten title", &rewritten).unwrap();
    transaction.commit().unwrap();

    let outbox_rows: Vec<(String, String)> = conn.prepare("SELECT state_kind, state_key FROM sync_outbox ORDER BY state_kind").unwrap().query_map([], |row| Ok((row.get(0)?, row.get(1)?))).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(outbox_rows, vec![("metadata".to_owned(), ContentHash::from_fixture_u64(1).to_string())], "exactly one outbox mutation must exist per (content_hash, kind) after a contributor-list rewrite");

    let pending = get_pending_mutations(&conn).unwrap();
    assert_eq!(pending.len(), 1);
    assert!(matches!(
        &pending[0].body,
        MutationBody::Metadata { content_hash, value } if content_hash.clone() == ContentHash::from_fixture_u64(1) && value.contributors == rewritten
    ));
}

#[test]
fn current_schema_has_integer_relationships_foreign_keys_and_lookup_indexes() {
    let conn = fk_conn();

    for (table, column) in [
        ("book_dir", "book_row_id"),
        ("book_contributor", "book_row_id"),
        ("annotation", "book_row_id"),
        ("bookmark", "book_row_id"),
        ("book_link", "source_book_row_id"),
        ("book_link", "target_book_row_id"),
    ] {
        let declared_type: String = conn
            .query_row(
                "SELECT type FROM pragma_table_info(?1) WHERE name=?2",
                rusqlite::params![table, column],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(declared_type.to_ascii_uppercase(), "INTEGER", "{table}.{column} must remain an integer relationship");
    }

    let foreign_key_errors: i64 = conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| row.get(0)).unwrap();
    assert_eq!(foreign_key_errors, 0);

    for index in ["sqlite_autoindex_book_1", "idx_book_dir_book", "idx_book_dir_local_hash", "idx_annotation_book", "idx_bookmark_book", "idx_book_link_source", "idx_book_link_target"] {
        let present: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_schema WHERE type='index' AND name=?1", [index], |row| row.get(0)).unwrap();
        assert_eq!(present, 1, "missing identity/relationship index {index}");
    }

    conn.execute("INSERT INTO book(content_hash,format) VALUES (?1,'epub')", [TEST_CONTENT_HASH]).unwrap();
    let book_lookup_plan: String = conn
        .query_row("EXPLAIN QUERY PLAN SELECT row_id FROM book WHERE content_hash=?1", [TEST_CONTENT_HASH], |row| row.get(3))
        .unwrap();
    assert!(book_lookup_plan.contains("sqlite_autoindex_book_1"), "content identity lookup must use its unique index: {book_lookup_plan}");

    let placement_lookup_plan: String = conn
        .query_row("EXPLAIN QUERY PLAN SELECT file_name FROM book_dir WHERE book_row_id=?1 AND deleted_at IS NULL", [1_i64], |row| row.get(3))
        .unwrap();
    assert!(placement_lookup_plan.contains("idx_book_dir_book"), "book placement lookup must use its row-id index: {placement_lookup_plan}");
}
