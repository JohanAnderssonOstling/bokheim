use super::*;

fn fixture() -> (Connection, ContentHash, BookRecord) {
    let mut conn = Connection::open_in_memory().unwrap();
    init_db(&conn).unwrap();
    let hash = test_support::fixture_content_hash(991);
    let mut metadata = BookRecord { title: "Original title".into(), description: "Original description".into(), contributors: Vec::new(), book: Default::default() };
    metadata.contributors.push(Contributor::new("An Author", None, MarcRelatorCode(*b"aut")).unwrap());
    metadata.book.subjects.push(BookSubject::new(None, "History", "test", Some("bisac".into()), Some("HIS000000".into())).unwrap());
    metadata.book.publishers.push(PublisherCredit::new("A Publisher").unwrap());
    metadata.book.languages.push(book_model::LanguageTag::parse("en").unwrap());
    metadata.book.identifiers.push(book_model::Identifier::new("9780192854216", Scheme::Isbn, Scope::Edition, "test").unwrap());
    metadata.book.rights.push("Copyright".into());
    let tx = conn.transaction().unwrap();
    insert_scanned_book_metadata(&tx, hash, &metadata, BookFormat::Epub).unwrap();
    tx.commit().unwrap();
    conn.execute_batch(include_str!("sql/metadata_delta_tests/fixture_create.sql")).unwrap();
    conn.execute_batch(include_str!("sql/delta_tests/fixture_query.sql")).unwrap();
    (conn, hash, metadata)
}
fn apply(conn: &mut Connection, hash: ContentHash, metadata: &BookRecord) {
    let tx = conn.transaction().unwrap();
    super::metadata_batch::run(&tx, || {
        update_book_metadata_state_preserving_ids(&tx, hash, &metadata.clone().into())?;
        update_book_description(&tx, hash, &metadata.description)
    })
    .unwrap();
    tx.commit().unwrap();
}
fn writes(conn: &Connection, table: &str) -> i64 {
    conn.query_row(include_str!("sql/metadata_delta_tests/writes_select.sql"), [table], |r| r.get(0)).unwrap()
}
fn assert_only(conn: &Connection, allowed: &[&str]) {
    let tables = conn.prepare(include_str!("sql/metadata_delta_tests/assert_only_select.sql")).unwrap().query_map([], |r| r.get::<_, String>(0)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();
    for table in tables {
        assert!(allowed.contains(&table.as_str()), "unexpected rewrite of {table}");
    }
}
#[test]
fn identical_metadata_is_a_database_noop() {
    let (mut conn, hash, metadata) = fixture();
    let before = conn.total_changes();
    apply(&mut conn, hash, &metadata);
    assert_only(&conn, &[]);
    assert_eq!(conn.total_changes(), before);
}
#[test]
fn title_and_description_share_one_update_without_projection_writes() {
    let (mut conn, hash, mut metadata) = fixture();
    metadata.title = "New title".into();
    metadata.description = "New description".into();
    apply(&mut conn, hash, &metadata);
    assert_eq!(writes(&conn, "book"), 1);
    assert_only(&conn, &["book"]);
}

#[test]
fn title_only_update_does_not_target_book_column() {
    let (mut conn, hash, mut metadata) = fixture();
    let before: Vec<u8> = conn.query_row(include_str!("sql/metadata/stored_book_select.sql"), [hash.as_str()], |r| r.get(0)).unwrap();
    conn.execute_batch(include_str!("sql/metadata_delta_tests/title_only_update_does_not_target_book_column_create.sql")).unwrap();
    metadata.title = "Only a new title".into();
    apply(&mut conn, hash, &metadata);
    assert_only(&conn, &["book"]);
    assert_eq!(writes(&conn, "book"), 1);
    let after: Vec<u8> = conn.query_row(include_str!("sql/metadata/stored_book_select.sql"), [hash.as_str()], |r| r.get(0)).unwrap();
    assert_eq!(before, after);
}

#[test]
fn contributor_comparison_preserves_all_ordered_credit_fields() {
    for variant in 0..6 {
        let (mut conn, hash, mut state) = fixture();
        state.contributors.push(Contributor::new("Second Author", None, MarcRelatorCode(*b"aut")).unwrap());
        apply(&mut conn, hash, &state);
        let tx = conn.transaction().unwrap();
        assert!(crate::authors::contributors_match(&tx, hash, &state.contributors).unwrap());
        let first = state.contributors[0].clone();
        match variant {
            0 => state.contributors.swap(0, 1),
            1 => state.contributors[0] = Contributor::with_id(first.agent_id(), "Changed name", None, first.role_code()).unwrap(),
            2 => state.contributors[0] = Contributor::with_id(first.agent_id(), first.name(), Some("Sort, Author".into()), first.role_code()).unwrap(),
            3 => state.contributors[0] = Contributor::with_id(first.agent_id(), first.name(), None, MarcRelatorCode(*b"edt")).unwrap(),
            4 => state.contributors[0] = Contributor::with_id(state.contributors[1].agent_id(), first.name(), None, first.role_code()).unwrap(),
            _ => state.contributors.clear(),
        }
        assert!(!crate::authors::contributors_match(&tx, hash, &state.contributors).unwrap());
        update_book_metadata_state_preserving_ids(&tx, hash, &state.clone().into()).unwrap();
        assert!(crate::authors::contributors_match(&tx, hash, &state.contributors).unwrap());
        tx.commit().unwrap();
    }
}

fn before_contributor_fast_path(tx: &Transaction, hash: ContentHash, metadata: &SyncBookMetadata) -> rusqlite::Result<()> {
    super::metadata_batch::run(tx, || {
        let previous = stored_book(tx, hash)?;
        let contributors = prepare_contributors(tx, &metadata.contributors, AuthorIdPolicy::PreserveIncoming, Some(hash))?;
        assert_eq!(previous.as_ref(), Some(&metadata.book));
        tx.prepare_cached(include_str!("sql/metadata_delta_tests/before_contributor_fast_path_update.sql"))?.execute(rusqlite::params![hash.as_str(), metadata.title])?;
        replace_book_contributors(tx, hash, &contributors)
    })
}

// Previous implementation retained only for an apples-to-apples benchmark.
#[test]
#[ignore = "manual release unchanged contributor comparison"]
fn benchmark_unchanged_contributors() {
    for (authors, changed) in [(0, false), (1, false), (4, false), (8, false), (4, true)] {
        let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
        let (_, _, mut state) = fixture();
        state.contributors = (0..authors).map(|i| Contributor::new(&format!("Author {i}"), None, MarcRelatorCode(*b"aut")).unwrap()).collect();
        let hashes = (1..=32).map(test_support::fixture_content_hash).collect::<Vec<_>>();
        let mut conns = Vec::new();
        for root in &roots {
            let mut conn = Connection::open(root.path().join("library.db")).unwrap();
            init_db(&conn).unwrap();
            let tx = library_write_transaction(&mut conn).unwrap();
            for hash in &hashes {
                insert_scanned_book_metadata(&tx, *hash, &state, BookFormat::Epub).unwrap();
            }
            tx.execute(include_str!("sql/metadata_delta_tests/benchmark_unchanged_contributors_delete.sql"), []).unwrap();
            tx.commit().unwrap();
            conns.push(conn);
        }
        let mut metadata: SyncBookMetadata = state.into();
        let mut times = [Vec::new(), Vec::new()];
        for trial in 0..15 {
            metadata.title = format!("Updated title {trial}");
            if changed {
                let last = metadata.contributors.last_mut().unwrap();
                *last = Contributor::with_id(last.agent_id(), &format!("Changed author {trial}"), None, last.role_code()).unwrap();
            }
            for mode in if trial % 2 == 0 { [0, 1] } else { [1, 0] } {
                let conn = &mut conns[mode];
                let start = std::time::Instant::now();
                let tx = library_write_transaction(conn).unwrap();
                crate::sync_state::set_sync_metadata(&tx, "change_origin", "remote").unwrap();
                for hash in &hashes {
                    if mode == 0 {
                        before_contributor_fast_path(&tx, *hash, &metadata).unwrap();
                    } else {
                        update_book_metadata_state_preserving_ids(&tx, *hash, &metadata).unwrap();
                    }
                }
                crate::sync_state::set_sync_metadata(&tx, "change_origin", "local").unwrap();
                tx.commit().unwrap();
                times[mode].push(start.elapsed());
                assert_eq!(conn.query_row(include_str!("sql/metadata_delta_tests/benchmark_unchanged_contributors_select.sql"), [&metadata.title], |r| r.get::<_, i64>(0)).unwrap(), 32);
                assert!(crate::sync_state::get_pending_mutations(conn).unwrap().is_empty());
            }
        }
        for t in &mut times {
            t.sort();
        }
        eprintln!("32 incoming domain updates authors={authors} changed={changed} before={:?} after={:?} trials=15", times[0][7], times[1][7]);
    }
}

fn legacy_metadata_apply(tx: &Transaction, hash: ContentHash, metadata: &SyncBookMetadata) -> rusqlite::Result<()> {
    super::metadata_batch::run(tx, || {
        let previous = stored_book(tx, hash)?;
        let contributors = prepare_contributors(tx, &metadata.contributors, AuthorIdPolicy::PreserveIncoming, Some(hash))?;
        let book = if previous.as_ref() == Some(&metadata.book) { metadata.book.clone() } else { prepare_book_agents(tx, &metadata.book, AuthorIdPolicy::PreserveIncoming)? };
        let encoded = sync_common::wire::encode(&book).map_err(|e| invalid_text_column(0, e.to_string()))?;
        tx.execute(include_str!("sql/metadata_delta_tests/legacy_metadata_apply_update.sql"), rusqlite::params![hash.as_str(), metadata.title, encoded])?;
        replace_book_contributors(tx, hash, &contributors)?;
        replace_book_metadata_projections(tx, hash, &book, previous.as_ref())
    })
}

#[test]
#[ignore = "manual release unchanged-book metadata fast path comparison"]
fn benchmark_title_only_book_fast_path() {
    for extra_bytes in [0, 1024, 10240] {
        let roots = [tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap()];
        let (_, _, mut state) = fixture();
        if extra_bytes > 0 {
            state.book.rights = vec!["x".repeat(extra_bytes)];
        }
        let hashes = (1..=32).map(test_support::fixture_content_hash).collect::<Vec<_>>();
        let mut conns = Vec::new();
        for root in &roots {
            let mut conn = Connection::open(root.path().join("library.db")).unwrap();
            init_db(&conn).unwrap();
            let tx = library_write_transaction(&mut conn).unwrap();
            for hash in &hashes {
                insert_scanned_book_metadata(&tx, *hash, &state, BookFormat::Epub).unwrap();
            }
            tx.execute(include_str!("sql/metadata_delta_tests/benchmark_unchanged_contributors_delete.sql"), []).unwrap();
            tx.commit().unwrap();
            conns.push(conn);
        }
        let mut metadata: SyncBookMetadata = state.into();
        let mut times = [Vec::new(), Vec::new()];
        for trial in 0..15 {
            metadata.title = format!("Updated title {trial}");
            for mode in if trial % 2 == 0 { [0, 1] } else { [1, 0] } {
                let conn = &mut conns[mode];
                let start = std::time::Instant::now();
                let tx = library_write_transaction(conn).unwrap();
                crate::sync_state::set_sync_metadata(&tx, "change_origin", "remote").unwrap();
                for hash in &hashes {
                    if mode == 0 {
                        legacy_metadata_apply(&tx, *hash, &metadata).unwrap();
                    } else {
                        update_book_metadata_state_preserving_ids(&tx, *hash, &metadata).unwrap();
                    }
                }
                crate::sync_state::set_sync_metadata(&tx, "change_origin", "local").unwrap();
                tx.commit().unwrap();
                times[mode].push(start.elapsed());
                assert_eq!(conn.query_row(include_str!("sql/metadata_delta_tests/benchmark_unchanged_contributors_select.sql"), [&metadata.title], |r| r.get::<_, i64>(0)).unwrap(), 32);
                assert!(crate::sync_state::get_pending_mutations(conn).unwrap().is_empty());
            }
        }
        for t in &mut times {
            t.sort();
        }
        eprintln!("32 title-only incoming domain updates book_extra_bytes={extra_bytes} before={:?} after={:?} trials=15", times[0][7], times[1][7]);
    }
}
#[test]
fn changing_languages_does_not_rewrite_classifications_or_publishers() {
    let (mut conn, hash, mut metadata) = fixture();
    metadata.book.languages = vec![book_model::LanguageTag::parse("sv").unwrap()];
    apply(&mut conn, hash, &metadata);
    assert_only(&conn, &["book", "book_language"]);
    assert_eq!(writes(&conn, "book_language"), 2);
    assert_eq!(conn.query_row(include_str!("sql/metadata_delta_tests/changing_languages_does_not_rewrite_classifications_or_publishers_select.sql"), [], |r| r.get::<_, String>(0)).unwrap(), "sv");
}
#[test]
fn removing_subjects_clears_classifications_without_rewriting_other_fields() {
    let (mut conn, hash, mut metadata) = fixture();
    metadata.book.subjects.clear();
    apply(&mut conn, hash, &metadata);
    assert_only(&conn, &["book", "book_subject", "book_subject_assignment", "book_unified_concept"]);
    assert!(writes(&conn, "book_subject") > 0);
    for (table, sql) in [
        ("book_subject", include_str!("sql/delta_tests/removing_subjects_clears_classifications_without_rewriting_other_fields_query_book_subject.sql")),
        ("book_subject_assignment", include_str!("sql/delta_tests/removing_subjects_clears_classifications_without_rewriting_other_fields_query_book_subject_assignment.sql")),
        ("book_unified_concept", include_str!("sql/delta_tests/removing_subjects_clears_classifications_without_rewriting_other_fields_query_book_unified_concept.sql")),
    ] {
        assert_eq!(conn.query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap(), 0, "unexpected rows in {table}");
    }
}
#[test]
fn standalone_description_and_contributor_noops_do_not_fire_triggers() {
    let (mut conn, hash, metadata) = fixture();
    let tx = conn.transaction().unwrap();
    update_book_description(&tx, hash, &metadata.description).unwrap();
    update_book_metadata_contributors_preserving_ids(&tx, hash, &metadata.title, &metadata.contributors).unwrap();
    tx.commit().unwrap();
    assert_only(&conn, &[]);
}
#[test]
fn changing_rights_preserves_other_book_value_kinds() {
    let (mut conn, hash, mut metadata) = fixture();
    metadata.book.rights = vec!["Updated rights".into()];
    apply(&mut conn, hash, &metadata);
    assert_only(&conn, &["book", "book_value"]);
    assert_eq!(writes(&conn, "book_value"), 2);
}

#[test]
fn adding_unclassified_subject_preserves_existing_classification_rows() {
    let (mut conn, hash, mut metadata) = fixture();
    conn.execute_batch(include_str!("sql/delta_tests/adding_unclassified_subject_preserves_existing_classification_rows_query.sql")).unwrap();
    metadata.book.subjects.push(BookSubject::new(None, "Personal note", "test", None, None).unwrap());
    apply(&mut conn, hash, &metadata);
    assert_only(&conn, &["book", "book_subject"]);
    assert_eq!(writes(&conn, "book_subject"), 1);
    conn.execute(include_str!("sql/metadata_delta_tests/adding_unclassified_subject_preserves_existing_classification_rows_delete.sql"), []).unwrap();
    metadata.book.subjects.pop();
    apply(&mut conn, hash, &metadata);
    assert_only(&conn, &["book", "book_subject"]);
    assert_eq!(writes(&conn, "book_subject"), 1);
}

#[test]
fn subject_reordering_and_removal_match_a_fresh_projection() {
    let (mut conn, hash, mut metadata) = fixture();
    metadata.book.subjects.push(BookSubject::new(None, "Art", "test", Some("bisac".into()), Some("ART000000".into())).unwrap());
    apply(&mut conn, hash, &metadata);
    metadata.book.subjects.swap(0, 1);
    apply(&mut conn, hash, &metadata);
    metadata.book.subjects.pop();
    apply(&mut conn, hash, &metadata);
    let mut fresh = Connection::open_in_memory().unwrap();
    init_db(&fresh).unwrap();
    let tx = fresh.transaction().unwrap();
    insert_scanned_book_metadata(&tx, hash, &metadata, BookFormat::Epub).unwrap();
    tx.commit().unwrap();
    for (table, sql) in [
        ("book_subject", include_str!("sql/delta_tests/subject_reordering_and_removal_match_a_fresh_projection_query_book_subject.sql")),
        ("book_subject_code", include_str!("sql/delta_tests/subject_reordering_and_removal_match_a_fresh_projection_query_book_subject_code.sql")),
        ("book_subject_assignment", include_str!("sql/delta_tests/subject_reordering_and_removal_match_a_fresh_projection_query_book_subject_assignment.sql")),
        ("book_subject_assignment_evidence", include_str!("sql/delta_tests/subject_reordering_and_removal_match_a_fresh_projection_query_book_subject_assignment_evidence.sql")),
        ("book_unified_concept", include_str!("sql/delta_tests/subject_reordering_and_removal_match_a_fresh_projection_query_book_unified_concept.sql")),
        ("book_unified_concept_evidence", include_str!("sql/delta_tests/subject_reordering_and_removal_match_a_fresh_projection_query_book_unified_concept_evidence.sql")),
    ] {
        let read = |c: &Connection| {
            let mut stmt = c.prepare(sql).unwrap();
            let n = stmt.column_count();
            stmt.query_map([], |r| (0..n).map(|i| r.get::<_, rusqlite::types::Value>(i)).collect::<rusqlite::Result<Vec<_>>>()).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap()
        };
        assert_eq!(read(&conn), read(&fresh), "{table}");
    }
    assert!(!conn.prepare(include_str!("sql/metadata_delta_tests/subject_reordering_and_removal_match_a_fresh_projection_pragma.sql")).unwrap().exists([]).unwrap());
}
