use crate::Database;
use super::convergence_tests::{change, db, hash, metadata, present};
use library_replica::{BookLifecycleState, LibraryStateEntry, ReadingPositionState};
use sync_common::{PullStateResponse, ROOT_DIR_ID, SyncCursor};

fn page(mutations: Vec<sync_common::StateDelivery>) -> PullStateResponse {
    PullStateResponse { mutations, next_cursor: SyncCursor::default(), has_more: false }
}
fn rows(db: &Database) -> i64 {
    db.connection.query_row("SELECT count(*) FROM book", [], |r| r.get(0)).unwrap()
}

#[test]
fn fields_never_create_a_missing_book_and_its_lifecycle_does_not_recover_them() {
    let values = vec![
        metadata("en", 100, 1),
        change(LibraryStateEntry::Description { content_hash: hash(), value: "Description".into() }, 100, 2),
        change(LibraryStateEntry::Placement { dir_id: ROOT_DIR_ID, content_hash: hash(), present: true, origin_folder_id: None }, 100, 3),
        change(LibraryStateEntry::ReadingPosition { content_hash: hash(), value: ReadingPositionState { location: book_model::ReadingPosition::parse("epubcfi(/6/2)").unwrap(), progress: 12.0 } }, 100, 4),
        change(LibraryStateEntry::BookFacts { content_hash: hash(), value: library_replica::BookFacts { added_at: 1, format: book_model::BookFormat::Epub } }, 100, 5),
        change(LibraryStateEntry::BookToc { content_hash: hash(), value: Default::default() }, 100, 6),
        change(
            LibraryStateEntry::Annotation {
                annotation_id: "early-annotation".into(),
                value: book_model::AnnotationState { content_hash: hash(), detail: book_model::AnnotationDetail { anchor: book_model::AnnotationAnchor::epub_cfi("epubcfi(/6/2)"), exact_text: "Text".into(), style: book_model::AnnotationStyle::Underline, color: "#fff".into(), note: String::new(), created_at: 10, toc_ordinal: Some(0), progress: Some(10.0) }, modified_at: 20, deleted: false },
            },
            100,
            8,
        ),
        change(
            LibraryStateEntry::PdfReaderMetadata {
                content_hash: hash(),
                value: pdf_view_common::PdfReaderMetadata {
                    version: 1,
                    checksum: hash().to_string(),
                    pages: vec![pdf_view_common::PdfStoredPage { width: 100.0, height: 200.0, rotation: 0 }],
                    skippable_pages: None,
                    annotations: vec![],
                    dependencies: None,
                },
            },
            100,
            9,
        ),
    ];
    let replica = db();
    for value in values {
        replica.sync_commit_pull_response(&[], &page(vec![value])).unwrap();
        assert_eq!(rows(&replica), 0);
    }
    // Absent-book fields are discarded; the later lifecycle cannot recover them.
    replica.sync_commit_pull_response(&[], &page(vec![present(100, 7)])).unwrap();
    assert_eq!(rows(&replica), 1);
    let state: (Option<String>, Option<i64>, f64) = replica.connection.query_row("SELECT title,format,read_progress FROM book", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!(state, (None, None, 0.0));
    assert!(replica.sync_publishable_mutations().unwrap().is_empty());
}

#[test]
fn a_lifecycle_creates_its_book_before_the_fields_on_the_same_page() {
    let replica = db();
    let cursor = SyncCursor { lifecycle_revision: sync_common::LibraryRevision::new(50).ok(), field_revision: sync_common::LibraryRevision::new(50).ok() };
    replica.set_sync_pull_cursor(&cursor).unwrap();
    let reading = change(LibraryStateEntry::ReadingPosition { content_hash: hash(), value: ReadingPositionState { location: book_model::ReadingPosition::parse("epubcfi(/6/2)").unwrap(), progress: 33.0 } }, 100, 52);
    let mut response = page(vec![present(1, 51), reading]);
    response.next_cursor = SyncCursor { lifecycle_revision: sync_common::LibraryRevision::new(52).ok(), field_revision: sync_common::LibraryRevision::new(52).ok() };
    sync_common::validate_pull_batch(&response, cursor).unwrap();
    replica.sync_commit_pull_response(&[], &response).unwrap();
    assert_eq!(rows(&replica), 1);
    assert_eq!(replica.sync_pull_cursor().unwrap(), response.next_cursor);
    replica.sync_commit_pull_response(&[], &response).unwrap();
    assert_eq!(rows(&replica), 1);
    assert!(replica.sync_publishable_mutations().unwrap().is_empty());
}

#[test]
fn retained_present_cannot_recreate_a_physically_removed_row() {
    let replica = db();
    replica.sync_commit_pull_response(&[], &page(vec![present(1, 1)])).unwrap();
    replica.connection.execute_batch(" DELETE FROM book; ").unwrap();
    replica.sync_commit_pull_response(&[], &page(vec![metadata("fr", 100, 2)])).unwrap();
    assert_eq!(rows(&replica), 0);
}

#[test]
fn a_trash_lifecycle_creates_a_deleted_row_and_a_purge_removes_it() {
    let replica = db();
    let trash = change(LibraryStateEntry::BookLifecycle { content_hash: hash(), value: BookLifecycleState::Deleted { origin_folder_id: None } }, 200, 3);
    replica.sync_commit_pull_response(&[], &page(vec![trash, metadata("en", 100, 2)])).unwrap();
    let deleted: bool = replica.connection.query_row("SELECT deleted_at IS NOT NULL FROM book", [], |r| r.get(0)).unwrap();
    assert!(deleted);
    replica.sync_commit_pull_response(&[], &page(vec![change(LibraryStateEntry::BookLifecycle { content_hash: hash(), value: BookLifecycleState::Purged }, 300, 4)])).unwrap();
    assert_eq!(rows(&replica), 0);
}

#[test]
fn stale_creation_cannot_override_purge_in_same_or_previous_page() {
    for separate_page in [false, true] {
        let replica = db();
        let purge = change(LibraryStateEntry::BookLifecycle { content_hash: hash(), value: BookLifecycleState::Purged }, 300, 3);
        let mut response = page(vec![present(100, 1), metadata("en", 200, 2)]);
        if separate_page {
            replica.sync_commit_pull_response(&[], &page(vec![purge])).unwrap();
        } else {
            response.mutations.insert(1, purge);
        }
        replica.sync_commit_pull_response(&[], &response).unwrap();
        assert_eq!(rows(&replica), 0);
        replica.sync_commit_pull_response(&[], &page(vec![present(400, 4)])).unwrap();
        assert_eq!(rows(&replica), 1);
        assert!(replica.sync_publishable_mutations().unwrap().is_empty());
    }
}

#[test]
fn missed_purge_readd_republishes_acknowledged_annotation_with_original_version() {
    for deleted in [false, true] {
        let annotation = change(
            LibraryStateEntry::Annotation {
                annotation_id: "surviving-note".into(),
                value: book_model::AnnotationState { content_hash: hash(), detail: book_model::AnnotationDetail { anchor: book_model::AnnotationAnchor::epub_cfi("epubcfi(/6/2)"), exact_text: "Text".into(), style: book_model::AnnotationStyle::Highlight, color: "#fff".into(), note: "Acknowledged before purge".into(), created_at: 1, toc_ordinal: None, progress: None }, modified_at: 2, deleted: deleted },
            },
            10,
            2,
        );
        let offline = db();
        let other = db();
        let initial = page(vec![present(1, 1), annotation.clone()]);
        for replica in [&offline, &other] {
            replica.sync_commit_pull_response(&[], &initial).unwrap();
            assert!(replica.sync_publishable_mutations().unwrap().is_empty());
        }
        other.sync_commit_pull_response(&[], &page(vec![change(LibraryStateEntry::BookLifecycle { content_hash: hash(), value: BookLifecycleState::Purged }, 20, 3)])).unwrap();
        let readd = page(vec![present(30, 4)]);
        for replica in [&offline, &other] {
            replica.sync_commit_pull_response(&[], &readd).unwrap();
        }
        let queued = offline.sync_publishable_mutations().unwrap();
        assert_eq!(queued.len(), 1, "acknowledged surviving annotation must be reconciled");
        let publication = &queued[0];
        let publisher = uuid::Uuid::from_u128(99);
        let wire = sync_common::transport::representation::<_, sync_common::OpaquePublication>(&(publication)).unwrap();
        assert_eq!((wire).state.version.clone(), annotation.state.version.clone());
        assert_eq!(offline.sync_publishable_mutations().unwrap(), queued, "retries preserve publication identity");
        let echoed = sync_common::fixture_delivery(wire, publisher, sync_common::LibraryRevision::new(5).unwrap());
        let reconciled = page(vec![present(30, 4), echoed]);
        let fresh = db();
        for replica in [&offline, &other, &fresh] {
            replica.sync_commit_pull_response(&[publication.publisher_seq], &reconciled).unwrap();
            let annotations = replica.annotations(hash()).unwrap();
            assert_eq!(annotations.len(), usize::from(!deleted));
            if let Some(annotation) = annotations.first() {
                assert_eq!(annotation.note, "Acknowledged before purge");
            }
            replica.sync_commit_pull_response(&[], &readd).unwrap();
            assert!(replica.sync_publishable_mutations().unwrap().is_empty(), "a repeated lifecycle or echo must not start another publication");
        }
    }
}

#[test]
fn reconciliation_keeps_local_winner_but_does_not_echo_server_confirmed_fields() {
    for remote_time in [5, 10, 40] {
        let replica = db();
        let old = metadata("en", 10, 2);
        replica.sync_commit_pull_response(&[], &page(vec![present(1, 1), old.clone()])).unwrap();
        let remote = if remote_time == 10 { old.clone() } else { metadata("fr", remote_time, 3) };
        let response = page(vec![present(30, 4), remote]);
        replica.sync_commit_pull_response(&[], &response).unwrap();
        let pending = replica.sync_publishable_mutations().unwrap();
        assert_eq!(pending.len(), usize::from(remote_time < 10));
        if let Some(publication) = pending.first() {
            assert_eq!((sync_common::transport::representation::<_, sync_common::OpaquePublication>(&(publication)).unwrap()).state.version.clone(), old.state.version.clone());
        }
    }
}

#[test]
fn reconciliation_is_atomic_durable_and_does_not_replace_pending_work() {
    let path = std::env::temp_dir().join(format!("missed-cycle-{}.sqlite3", uuid::Uuid::new_v4()));
    let replica = Database::open(&path).unwrap();
    replica.initialize_library().unwrap();
    replica.sync_commit_pull_response(&[], &page(vec![present(1, 1), metadata("en", 10, 2)])).unwrap();
    let response = page(vec![present(30, 4)]);
    replica.connection.execute_batch("CREATE TEMP TRIGGER reject_recovery BEFORE INSERT ON sync_outbox BEGIN SELECT RAISE(ABORT,'injected recovery failure'); END;").unwrap();
    assert!(replica.sync_commit_pull_response(&[], &response).is_err());
    assert_eq!(replica.connection.query_row("SELECT changed_at FROM sync_state_version WHERE state_kind='book_lifecycle'", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    replica.connection.execute_batch("DROP TRIGGER reject_recovery;").unwrap();
    replica.sync_commit_pull_response(&[], &response).unwrap();
    let pending = replica.sync_publishable_mutations().unwrap();
    assert_eq!(pending.len(), 1);
    drop(replica);
    let replica = Database::open(&path).unwrap();
    replica.initialize_library().unwrap();
    assert_eq!(replica.sync_publishable_mutations().unwrap(), pending);
    replica.sync_commit_pull_response(&[], &page(vec![present(40, 5)])).unwrap();
    assert_eq!(replica.sync_publishable_mutations().unwrap(), pending, "another lifecycle must preserve the pending publication identity");
    replica.sync_commit_pull_response(&[], &page(vec![change(LibraryStateEntry::BookLifecycle { content_hash: hash(), value: BookLifecycleState::Purged }, 50, 6)])).unwrap();
    assert!(replica.sync_publishable_mutations().unwrap().is_empty(), "a later purge cancels reconciliation");
    drop(replica);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn locally_authored_existence_reconciles_fields_before_its_equal_version_echo() {
    let replica = db();
    let old = metadata("en", 10, 2);
    replica.sync_commit_pull_response(&[], &page(vec![present(1, 1), old.clone()])).unwrap();
    replica.trash_book(&hash()).unwrap();
    let pending = replica.sync_publishable_mutations().unwrap();
    let field = pending.iter().find(|m| matches!(m.state.value, LibraryStateEntry::Metadata { .. })).expect("local existence change queues acknowledged fields too");
    let publisher = uuid::Uuid::parse_str(&replica.connection.query_row("SELECT replica_id FROM sync_metadata", [], |r| r.get::<_, String>(0)).unwrap()).unwrap();
    assert_eq!((sync_common::transport::representation::<_, sync_common::OpaquePublication>(&(field)).unwrap()).state.version.clone(), old.state.version.clone());
    let echoed = pending.iter().enumerate().map(|(i, m)| sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&(m)).unwrap(), publisher, sync_common::LibraryRevision::new(i as u64 + 1).unwrap())).collect::<Vec<_>>();
    replica.sync_commit_pull_response(&pending.iter().map(|m| m.publisher_seq).collect::<Vec<_>>(), &page(echoed)).unwrap();
    assert!(replica.sync_publishable_mutations().unwrap().is_empty());
}

#[test]
fn acknowledgement_confirms_only_its_matching_field_generation() {
    for superseded in [false, true] {
        let replica = db();
        replica.sync_commit_pull_response(&[], &page(vec![present(1, 1), metadata("en", 10, 2)])).unwrap();
        replica.sync_enqueue_missing_state_cells(&[sync_common::StateAddress { kind: "metadata".into(), entity_key: hash().to_string(), entity_subkey: String::new() }]).unwrap();
        let sent = replica.sync_publishable_mutations().unwrap();
        if superseded {
            replica.sync_commit_pull_response(&[], &page(vec![metadata("fr", 20, 3)])).unwrap();
        }
        let pending_before = replica.sync_publishable_mutations().unwrap();
        replica.sync_commit_pull_response(&[sent[0].publisher_seq], &page(vec![present(30, 4)])).unwrap();
        let after = replica.sync_publishable_mutations().unwrap();
        if superseded {
            assert_eq!(after, pending_before, "an old acknowledgement cannot confirm a newer field");
        } else {
            assert!(after.is_empty(), "an acknowledged field need not wait for its pull page to be confirmed");
        }
    }
}
