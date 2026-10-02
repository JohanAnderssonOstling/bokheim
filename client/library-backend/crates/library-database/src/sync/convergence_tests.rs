//! Compatibility fixtures: compare canonical winners and their visible projection.
use crate::{Database, sync::apply};
use library_replica::{BookDetails, BookLifecycleState, DirectoryLifecycleState, LibraryStateEntry, ReadingPositionState};
use rusqlite::OptionalExtension;
use sync_common::{ContentHash, LibraryRevision, PullStateResponse, ROOT_DIR_ID, ReplicaSeq, StateDelivery, SyncCursor};

pub(super) fn db() -> Database {
    let db = Database::open(":memory:").unwrap();
    db.initialize_library().unwrap();
    db
}
pub(super) fn change(body: LibraryStateEntry, time: u64, seq: u64) -> StateDelivery {
    let publication = body.clone().publication(sync_common::ReplicaId::nil(), time, ReplicaSeq::new(seq).unwrap());
    let opaque = sync_common::Publication {
        publisher_seq: publication.publisher_seq,
        state: sync_common::Versioned { version: publication.state.version, value: sync_common::OpaqueStateEntry { address: body.address(), book: body.book(), bytes: sync_common::transport::encode(&body).unwrap() } },
    };
    sync_common::fixture_delivery(opaque, uuid::Uuid::from_u128(42), LibraryRevision::new(seq).unwrap())
}
pub(super) fn pull(db: &Database, changes: &[StateDelivery]) {
    db.commit_existing_books_fixture(&[], &PullStateResponse { mutations: changes.to_vec(), next_cursor: SyncCursor::default(), has_more: false }).unwrap();
}
pub(super) fn hash() -> ContentHash {
    ContentHash::new(&"a".repeat(64))
}
pub(super) fn present(time: u64, seq: u64) -> StateDelivery {
    change(LibraryStateEntry::BookLifecycle { content_hash: hash(), value: BookLifecycleState::Present }, time, seq)
}
pub(super) fn metadata(language: &str, time: u64, seq: u64) -> StateDelivery {
    let mut value = BookDetails::default();
    value.title = "Title".into();
    value.book.languages = vec![book_model::LanguageTag::parse(language).unwrap()];
    change(LibraryStateEntry::Metadata { content_hash: hash(), value }, time, seq)
}
fn book_snapshot(db: &Database) -> (Option<String>, Option<String>, Vec<String>, bool) {
    let row = db.connection.query_row("SELECT title,read_pos FROM book WHERE content_hash=?1", [hash().as_str()], |r| Ok((r.get(0)?, r.get(1)?))).optional().unwrap();
    let Some((title, pos)) = row else {
        let (bytes, _) = apply::registers::canonical_body(&db.connection, "book_lifecycle", hash().as_str(), "").unwrap().expect("missing book requires a known purge in this fixture");
        let body: LibraryStateEntry = sync_common::wire::decode(&bytes, sync_common::wire::MAX_DECODED_REQUEST_BYTES).unwrap();
        assert!(matches!(body, LibraryStateEntry::BookLifecycle { value: BookLifecycleState::Purged, .. }));
        return (None, None, vec![], true);
    };
    let languages = db.connection.prepare("SELECT language FROM book_language ORDER BY language").unwrap().query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
    (title, pos, languages, false)
}
#[test]
fn purge_discards_fields_and_only_readd_admits_updates() {
    // Purge removes earlier fields, and updates received while absent are ignored.
    for value_time in [15, 25] {
        let meta = metadata("fr", value_time, 3);
        let reading = change(LibraryStateEntry::ReadingPosition { content_hash: hash(), value: ReadingPositionState { location: book_model::ReadingPosition::parse("epubcfi(/6/2)").unwrap(), progress: 0.5 } }, value_time, 4);
        let purge = change(LibraryStateEntry::BookLifecycle { content_hash: hash(), value: BookLifecycleState::Purged }, 20, 2);
        for events in [vec![meta.clone(), reading.clone(), purge.clone()], vec![purge, reading, meta]] {
            let replica = db();
            pull(&replica, &[present(10, 1)]);
            for event in &events {
                pull(&replica, std::slice::from_ref(event));
            }
            assert_eq!(book_snapshot(&replica), (None, None, vec![], true), "register updates cannot revive a purge");
            pull(&replica, &[present(30, 5)]);
            assert_eq!(book_snapshot(&replica), (None, None, vec![], false));
            pull(&replica, &events);
            assert_eq!(book_snapshot(&replica), (Some("Title".into()), Some("epubcfi(/6/2)".into()), vec!["fr".into()], false), "replay preserves the re-add");
        }
    }
}
#[test]
fn purge_reimport_converges_for_page_boundaries_and_fresh_snapshot() {
    // The server removes pre-purge fields. Its current snapshot contains only
    // the readd and fields accepted after that readd, even with older clocks.
    let reading = change(LibraryStateEntry::ReadingPosition { content_hash: hash(), value: ReadingPositionState { location: book_model::ReadingPosition::parse("epubcfi(/6/2)").unwrap(), progress: 0.5 } }, 15, 3);
    let meta = metadata("fr", 16, 4);
    let revived = present(30, 5);
    let snapshot = [revived.clone(), reading, meta];
    for page_size in [1, 2, 3] {
        for previously_present in [false, true] {
            let replica = db();
            if previously_present {
                pull(&replica, &[present(10, 1), metadata("en", 11, 2)]);
                pull(&replica, &[change(LibraryStateEntry::BookLifecycle { content_hash: hash(), value: BookLifecycleState::Purged }, 20, 20)]);
            }
            for page in snapshot.chunks(page_size) {
                let response = PullStateResponse { mutations: page.to_vec(), next_cursor: SyncCursor::default(), has_more: false };
                replica.sync_commit_pull_response(&[], &response).unwrap();
                replica.sync_commit_pull_response(&[], &response).unwrap();
            }
            assert_eq!(book_snapshot(&replica), (Some("Title".into()), Some("epubcfi(/6/2)".into()), vec!["fr".into()], false));
        }
    }
}
#[test]
fn languages_and_optional_toc_fields_replace_the_previous_winner() {
    let toc = |duration, seq| {
        change(
            LibraryStateEntry::BookToc {
                content_hash: hash(),
                value: book_model::BookTocDocument {
                    entries: vec![],
                    duration_ms: duration,
                    tracks: duration.map(|end_ms| vec![book_model::AudiobookTrack { name: "one.mp3".into(), start_ms: 0, end_ms, offset: 0, length: 20, archive_checksum: None }]),
                },
            },
            seq,
            seq,
        )
    };
    let old = toc(Some(1000), 3);
    let new = toc(None, 4);
    let left = db();
    pull(&left, &[present(1, 1), metadata("en", 2, 2), old]);
    pull(&left, &[metadata("fr", 5, 5), new.clone()]);
    let right = db();
    pull(&right, &[present(1, 1), metadata("fr", 5, 5), new]);
    assert_eq!(book_snapshot(&left), book_snapshot(&right));
    for db in [&left, &right] {
        assert!(db.audiobook_tracks(&hash()).unwrap().is_empty());
        assert_eq!(db.connection.query_row("SELECT COUNT(*) FROM book_navigation WHERE duration_ms IS NOT NULL", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    }
}
pub(super) fn folder_events() -> Vec<StateDelivery> {
    let mut events = Vec::new();
    for (id, parent, name) in [(1, 2, "Shelf"), (2, 1, "Cycle"), (3, 0, "cycle"), (4, 99, "Unreachable")] {
        let dir_id = uuid::Uuid::from_u128(id);
        for body in [
            LibraryStateEntry::DirectoryName { dir_id, name: name.into() },
            LibraryStateEntry::DirectoryParent { dir_id, parent_id: uuid::Uuid::from_u128(parent) },
            LibraryStateEntry::DirectoryLifecycle { dir_id, value: DirectoryLifecycleState::Present },
        ] {
            let seq = events.len() as u64 + 1;
            events.push(change(body, seq, seq));
        }
    }
    events
}
fn tree(db: &Database) -> Vec<(String, String, String, bool)> {
    db.connection.prepare("SELECT id,parent_id,name,deleted_at IS NOT NULL FROM dir ORDER BY id").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).unwrap().collect::<Result<_, _>>().unwrap()
}
#[test]
fn cycle_projection_and_recovery_preserve_original_intents() {
    let events = folder_events();
    let left = db();
    pull(&left, &events);
    let right = db();
    for event in events.iter().rev() {
        pull(&right, std::slice::from_ref(event));
    }
    assert_eq!(tree(&left), tree(&right));
    let cells = left.sync_inventory_page(None).unwrap();
    left.sync_enqueue_missing_state_cells(&cells).unwrap();
    let recovered = left.sync_publishable_mutations().unwrap();
    for event in &events {
        let original = LibraryStateEntry::decode(&event.state).unwrap().unwrap();
        assert!(recovered.iter().any(|r| r.state.value == original), "recovery changed {original:?}");
    }
    let fresh = db();
    let changes = recovered
        .into_iter()
        .map(|m| sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&(m)).unwrap(), uuid::Uuid::from_u128(51), LibraryRevision::new(1).unwrap()))
        .collect::<Vec<_>>();
    pull(&fresh, &changes);
    assert_eq!(tree(&left), tree(&fresh));
    // Recovery preserves both intent and stable name/cycle projection.
    assert_eq!(fresh.connection.query_row("SELECT canonical_parent_id FROM dir WHERE id=?1", [uuid::Uuid::from_u128(2).to_string()], |r| r.get::<_, String>(0)).unwrap(), uuid::Uuid::from_u128(1).to_string());
    assert_eq!(fresh.connection.query_row("SELECT lifecycle FROM dir WHERE id=?1", [uuid::Uuid::from_u128(4).to_string()], |r| r.get::<_, i64>(0)).unwrap(), 0);
}
#[test]
fn local_restore_survives_reprojection_and_matches_remote() {
    let local = db();
    let dir = uuid::Uuid::from_u128(1);
    local.create_directory_with_id(&dir, &ROOT_DIR_ID, &"Shelf".to_owned()).unwrap();
    local.trash_directory(&dir).unwrap();
    local.restore_directory(&dir, None).unwrap();
    // Reproject the folder tree from its registers, as any commit that
    // touches a directory register does.
    let tx = local.connection.unchecked_transaction().unwrap();
    tx.execute("INSERT OR IGNORE INTO sync_projection_dirty VALUES('directory_lifecycle',?1,'')", [dir.to_string()]).unwrap();
    crate::sync::apply::commit(tx).unwrap();
    assert_eq!(local.connection.query_row("SELECT lifecycle,deleted_at FROM dir WHERE id=?1", [dir.to_string()], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?))).unwrap(), (0, None));
    let remote = db();
    let changes = local
        .sync_publishable_mutations()
        .unwrap()
        .into_iter()
        .map(|m| sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&(m)).unwrap(), uuid::Uuid::from_u128(51), LibraryRevision::new(1).unwrap()))
        .collect::<Vec<_>>();
    pull(&remote, &changes);
    assert_eq!(tree(&local), tree(&remote));
}

#[test]
fn folder_collisions_converge_after_local_edits_and_cycle_repair() {
    let mut seed = folder_events();
    for (id, name) in [(5, "Target"), (6, "cycle 2")] {
        let dir_id = uuid::Uuid::from_u128(id);
        for body in
            [LibraryStateEntry::DirectoryName { dir_id, name: name.into() }, LibraryStateEntry::DirectoryParent { dir_id, parent_id: ROOT_DIR_ID }, LibraryStateEntry::DirectoryLifecycle { dir_id, value: DirectoryLifecycleState::Present }]
        {
            let seq = seed.len() as u64 + 1;
            seed.push(change(body, seq, seq));
        }
    }
    for operation in ["rename", "move", "trash", "restore"] {
        let local = db();
        pull(&local, &seed);
        let repaired = uuid::Uuid::from_u128(2);
        let winner = uuid::Uuid::from_u128(3);
        match operation {
            "rename" => {
                local.move_directory(&repaired, None, Some("cycle 2")).unwrap();
            }
            "move" => {
                local.move_directory(&repaired, Some(&uuid::Uuid::from_u128(5)), None).unwrap();
            }
            "trash" => {
                local.trash_directory(&winner).unwrap();
            }
            "restore" => {
                local.trash_directory(&winner).unwrap();
                local.restore_directory(&winner, None).unwrap();
            }
            _ => unreachable!(),
        }
        let expected = tree(&local);
        let mut events = seed.clone();
        events.extend(
            local
                .sync_publishable_mutations()
                .unwrap()
                .into_iter()
                .map(|mutation| sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&(mutation)).unwrap(), uuid::Uuid::from_u128(51), LibraryRevision::new(1).unwrap())),
        );
        for order in 0..6 {
            let mut ordered = events.clone();
            if order == 1 {
                ordered.reverse();
            }
            if order >= 2 {
                let mut state = order as u64;
                for i in (1..ordered.len()).rev() {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    ordered.swap(i, (state % (i as u64 + 1)) as usize);
                }
            }
            for page_size in [1, 3, ordered.len()] {
                let remote = db();
                for page in ordered.chunks(page_size) {
                    pull(&remote, page);
                }
                assert_eq!(tree(&remote), expected, "{operation}, order={order}, page_size={page_size}");
                ordered.reverse();
                for page in ordered.chunks(page_size) {
                    pull(&remote, page);
                }
                ordered.reverse();
                assert_eq!(tree(&remote), expected, "replay after {operation}");
                assert!(remote.sync_publishable_mutations().unwrap().is_empty(), "projection must not publish repaired names or parents");
            }
        }
    }
}

#[test]
fn moving_a_collision_numbered_folder_returns_its_final_name() {
    let replica = db();
    pull(&replica, &folder_events());
    let target = uuid::Uuid::from_u128(5);
    replica.create_directory_with_id(&target, &ROOT_DIR_ID, &"Target".to_owned()).unwrap();
    let repaired = uuid::Uuid::from_u128(2);
    let returned = replica.move_directory(&repaired, Some(&target), None).unwrap();
    let (name, intent): (String, String) = replica.connection.query_row("SELECT name,canonical_name FROM dir WHERE id=?1", [repaired.to_string()], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(intent, "Cycle", "moving must preserve the original name intent");
    assert_eq!(name, "Cycle", "the destination has no collision");
    assert_eq!(returned, name, "the caller must receive the final projected name");
}

#[test]
fn local_purge_removes_metadata_and_reimport_starts_without_it() {
    let replica = db();
    pull(&replica, &[present(10, 1), metadata("fr", 11, 2)]);
    replica.connection.execute("INSERT INTO book_transfer(content_hash,server_has_book) VALUES(?1,1)", [hash().as_str()]).unwrap();
    replica.trash_book(&hash()).unwrap();
    replica.purge_book(&hash()).unwrap();
    assert_eq!(replica.connection.query_row("SELECT COUNT(*) FROM book_transfer WHERE content_hash=?1", [hash().as_str()], |r| r.get::<_, i64>(0)).unwrap(), 0);
    assert!(book_snapshot(&replica).3);
    assert!(apply::registers::canonical_body(&replica.connection, "metadata", hash().as_str(), "").unwrap().is_none());
    assert!(replica.sync_publishable_mutations().unwrap().iter().all(|m| matches!(m.state.value, LibraryStateEntry::BookLifecycle { .. })));
    let newer = replica.connection.query_row("SELECT changed_at+1 FROM sync_state_version WHERE state_kind='book_lifecycle'", [], |r| r.get::<_, i64>(0)).unwrap() as u64;
    pull(&replica, &[present(newer, 50)]);
    assert_eq!(book_snapshot(&replica), (None, None, vec![], false));
}

#[test]
fn equal_version_refetch_fills_a_legacy_missing_payload() {
    let replica = db();
    let event = metadata("fr", 10, 1);
    pull(&replica, std::slice::from_ref(&event));
    replica.connection.execute("UPDATE sync_state_version SET body=NULL WHERE state_kind='metadata'", []).unwrap();
    pull(&replica, &[event]);
    assert!(apply::registers::canonical_body(&replica.connection, "metadata", hash().as_str(), "").unwrap().is_some());
}

#[test]
fn failed_projection_rolls_back_canonical_winners_and_local_outbox() {
    let replica = db();
    let dir = uuid::Uuid::from_u128(5);
    replica.create_directory_with_id(&dir, &ROOT_DIR_ID, &"Before".into()).unwrap();
    let before = apply::registers::canonical_body(&replica.connection, "directory_name", &dir.to_string(), "").unwrap();
    let outbox = replica.sync_publishable_mutations().unwrap();
    replica.connection.execute_batch("CREATE TEMP TRIGGER reject_projection BEFORE UPDATE OF canonical_name ON dir BEGIN SELECT RAISE(ABORT,'projection failure'); END;").unwrap();
    assert!(replica.move_directory(&dir, None, Some("After")).is_err());
    assert_eq!(apply::registers::canonical_body(&replica.connection, "directory_name", &dir.to_string(), "").unwrap(), before);
    assert_eq!(replica.sync_publishable_mutations().unwrap(), outbox);
    assert_eq!(replica.connection.query_row("SELECT name FROM dir WHERE id=?1", [dir.to_string()], |r| r.get::<_, String>(0)).unwrap(), "Before");
}

#[test]
fn invalid_values_are_rejected_even_while_purged() {
    let replica = db();
    pull(&replica, &[change(LibraryStateEntry::BookLifecycle { content_hash: hash(), value: BookLifecycleState::Purged }, 10, 1)]);
    let invalid = change(LibraryStateEntry::BookToc { content_hash: hash(), value: book_model::BookTocDocument { entries: vec![], duration_ms: Some(10), tracks: Some(vec![]) } }, 11, 2);
    assert!(replica.sync_commit_pull_response(&[], &PullStateResponse { mutations: vec![invalid], next_cursor: SyncCursor::default(), has_more: false }).is_err());
    assert!(apply::registers::canonical_body(&replica.connection, "book_toc", hash().as_str(), "").unwrap().is_none());
}

#[test]
fn local_clock_rollback_still_publishes_the_visible_edit() {
    let replica = db();
    let old = change(LibraryStateEntry::ReadingPosition { content_hash: hash(), value: ReadingPositionState { location: book_model::ReadingPosition::parse("epubcfi(/6/2)").unwrap(), progress: 0.1 } }, 200, 1);
    pull(&replica, &[present(10, 2), old]);
    let body = LibraryStateEntry::ReadingPosition { content_hash: hash(), value: ReadingPositionState { location: book_model::ReadingPosition::parse("epubcfi(/6/4)").unwrap(), progress: 0.2 } };
    replica.enqueue_outbox_reading_change(hash().as_str(), &sync_common::wire::encode(&body).unwrap(), 100).unwrap();
    let queued = replica.sync_publishable_mutations().unwrap();
    assert_eq!(queued[0].state.version.changed_at, 201);
    assert_eq!(queued[0].state.value, body);
    assert_eq!(replica.reading_position(&hash()).unwrap().as_deref(), Some("epubcfi(/6/4)"));
}

pub(super) fn facts(time: u64, seq: u64) -> StateDelivery {
    change(LibraryStateEntry::BookFacts { content_hash: hash(), value: library_replica::BookFacts { added_at: 10, format: book_model::BookFormat::Epub } }, time, seq)
}

#[test]
fn independent_facts_allow_fresh_device_to_restore_deleted_book() {
    let old = db();
    let fresh = db();
    let deleted = change(LibraryStateEntry::BookLifecycle { content_hash: hash(), value: BookLifecycleState::Deleted { origin_folder_id: None } }, 20, 3);
    let placement = change(LibraryStateEntry::Placement { content_hash: hash(), dir_id: ROOT_DIR_ID, present: true, origin_folder_id: None }, 21, 4);
    pull(&old, &[facts(10, 1), present(10, 2)]);
    pull(&old, &[deleted.clone(), placement.clone()]);
    pull(&fresh, &[deleted, placement, facts(10, 1)]);
    for replica in [&old, &fresh] {
        let actual = replica.connection.query_row("SELECT format,added_at FROM book", [], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).unwrap();
        assert_eq!(actual, (book_model::BookFormat::Epub as i64, 10));
        replica.restore_book_commit(&hash(), &[(ROOT_DIR_ID, ROOT_DIR_ID)]).unwrap();
        assert!(replica.sync_publishable_mutations().unwrap().iter().any(|m| matches!(m.state.value, LibraryStateEntry::BookLifecycle { value: BookLifecycleState::Present, .. })));
    }
}

#[test]
fn recovery_replaces_stale_publication_and_old_ack_cannot_remove_it() {
    let replica = db();
    let dir = uuid::Uuid::from_u128(1);
    replica.create_directory_with_id(&dir, &ROOT_DIR_ID, &"Old".into()).unwrap();
    let old = replica.sync_publishable_mutations().unwrap();
    let time = old.iter().map(|m| m.state.version.changed_at).max().unwrap() + 100;
    pull(&replica, &[change(LibraryStateEntry::DirectoryName { dir_id: dir, name: "Winner".into() }, time, 100)]);
    replica.sync_enqueue_missing_state_cells(&replica.sync_inventory_page(None).unwrap()).unwrap();
    replica.sync_acknowledge_mutations(&old.iter().map(|m| m.publisher_seq).collect::<Vec<_>>()).unwrap();
    let recovered = replica.sync_publishable_mutations().unwrap();
    assert!(recovered.iter().any(|m| matches!(&m.state.value,LibraryStateEntry::DirectoryName {name,..} if name=="Winner")));
    assert_eq!(recovered, replica.sync_publishable_mutations().unwrap(), "retry snapshots must be immutable");
    let winner = recovered.iter().find(|m| matches!(m.state.value, LibraryStateEntry::DirectoryName { .. })).unwrap();
    assert_eq!(winner.state.version.changed_at, time, "recovery must not advance the logical version");
}

#[test]
fn publication_is_lazy_and_in_flight_ack_preserves_new_local_edit() {
    let replica = db();
    let dir = uuid::Uuid::from_u128(1);
    replica.create_directory_with_id(&dir, &ROOT_DIR_ID, &"First".into()).unwrap();
    let pending: i64 = replica.connection.query_row("SELECT COUNT(*) FROM sync_outbox WHERE body IS NULL", [], |r| r.get(0)).unwrap();
    assert_eq!(pending, 3, "a local edit queues identities, not copied payloads");
    let sent = replica.sync_publishable_mutations().unwrap();
    assert_eq!(sent, replica.sync_publishable_mutations().unwrap());
    replica.move_directory(&dir, None, Some("Second")).unwrap();
    replica.sync_acknowledge_mutations(&sent.iter().map(|m| m.publisher_seq).collect::<Vec<_>>()).unwrap();
    let next = replica.sync_publishable_mutations().unwrap();
    assert_eq!(next.len(), 1);
    assert!(matches!(&next[0].state.value,LibraryStateEntry::DirectoryName {name,..} if name=="Second"));
    assert!(!sent.iter().any(|m| m.publisher_seq == next[0].publisher_seq));
}

#[test]
fn complete_book_projection_does_not_inherit_previous_display_fields() {
    let old = db();
    let fresh = db();
    let final_lifecycle = change(LibraryStateEntry::BookLifecycle { content_hash: hash(), value: BookLifecycleState::Deleted { origin_folder_id: None } }, 40, 5);
    pull(&old, &[facts(10, 1), present(10, 2), metadata("fr", 11, 3)]);
    pull(&old, &[change(LibraryStateEntry::BookLifecycle { content_hash: hash(), value: BookLifecycleState::Purged }, 20, 4)]);
    pull(&old, &[final_lifecycle.clone()]);
    pull(&fresh, &[final_lifecycle]);
    let snapshot = |replica: &Database| {
        replica
            .connection
            .query_row("SELECT format,added_at,title,subtitle,read_pos,read_progress,deleted_at,trash_origin_dir_id,read_at FROM book", [], |r| {
                Ok((
                    r.get::<_, Option<i64>>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, f64>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, Option<String>>(7)?,
                    r.get::<_, Option<i64>>(8)?,
                ))
            })
            .unwrap()
    };
    assert_eq!(snapshot(&old), snapshot(&fresh));
    assert_eq!(book_snapshot(&old), book_snapshot(&fresh));
}

#[test]
fn publication_snapshot_survives_reopen() {
    let path = std::env::temp_dir().join(format!("publication-{}.sqlite3", uuid::Uuid::new_v4()));
    let replica = Database::open(&path).unwrap();
    replica.initialize_library().unwrap();
    replica.create_directory_with_id(&uuid::Uuid::from_u128(1), &ROOT_DIR_ID, &"Shelf".into()).unwrap();
    let sent = replica.sync_publishable_mutations().unwrap();
    drop(replica);
    let replica = Database::open(&path).unwrap();
    replica.initialize_library().unwrap();
    assert_eq!(sent, replica.sync_publishable_mutations().unwrap());
    replica.sync_acknowledge_mutations(&sent.iter().map(|m| m.publisher_seq).collect::<Vec<_>>()).unwrap();
    assert!(replica.sync_publishable_mutations().unwrap().is_empty());
    drop(replica);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn recovery_must_not_revive_a_purged_book() {
    let stale = db();
    let purged = db();
    let addition = present(100, 1);
    let deletion = change(LibraryStateEntry::BookLifecycle { content_hash: hash(), value: BookLifecycleState::Purged }, 100, 2);
    pull(&stale, &[addition.clone()]);
    pull(&purged, &[addition, deletion.clone()]);
    assert!(book_snapshot(&purged).3);
    let cells = stale.sync_inventory_page(None).unwrap();
    stale.sync_enqueue_missing_state_cells(&cells).unwrap();
    let recovered = stale.sync_publishable_mutations().unwrap();
    let replay = recovered
        .into_iter()
        .map(|m| sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&(m)).unwrap(), uuid::Uuid::from_u128(51), LibraryRevision::new(3).unwrap()))
        .collect::<Vec<_>>();
    pull(&purged, &replay);
    assert!(book_snapshot(&purged).3, "inventory repair must not revive a book without a user re-add");
}

#[test]
fn import_retry_must_accept_collision_repair() {
    let replica = db();
    let local = uuid::Uuid::from_u128(1);
    let other = uuid::Uuid::from_u128(2);
    replica.create_directory_with_id(&local, &ROOT_DIR_ID, &"Shelf".into()).unwrap();
    pull(
        &replica,
        &[
            change(LibraryStateEntry::DirectoryName { dir_id: other, name: "Shelf".into() }, 100, 10),
            change(LibraryStateEntry::DirectoryParent { dir_id: other, parent_id: ROOT_DIR_ID }, 100, 11),
            change(LibraryStateEntry::DirectoryLifecycle { dir_id: other, value: DirectoryLifecycleState::Present }, 100, 12),
        ],
    );
    let (name, intent): (String, String) = replica.connection.query_row("SELECT name,canonical_name FROM dir WHERE id=?1", [local.to_string()], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(name, "Shelf 2");
    assert_eq!(intent, "Shelf");
    let before = replica.sync_publishable_mutations().unwrap();
    let retried = replica.create_directory_with_id(&local, &ROOT_DIR_ID, &"Shelf".into()).expect("an unchanged import intent should remain retryable after repair");
    assert_eq!(retried.name, "Shelf 2");
    assert_eq!(replica.sync_publishable_mutations().unwrap(), before, "a retry must not publish the repaired name as an edit");
}

#[test]
fn recovery_preserves_the_full_winner_across_repeated_publication_and_reopen() {
    let path = std::env::temp_dir().join(format!("recovery-origin-{}.sqlite3", uuid::Uuid::new_v4()));
    let replica = Database::open(&path).unwrap();
    replica.initialize_library().unwrap();
    let original = metadata("fr", 100, 7);
    let expected = original.state.version.clone();
    replica.sync_commit_pull_response(&[], &PullStateResponse { mutations: vec![present(1, 1), original], next_cursor: SyncCursor::default(), has_more: false }).unwrap();
    let cells = replica.sync_inventory_page(None).unwrap().into_iter().filter(|cell| cell.kind == "metadata").collect::<Vec<_>>();
    for _ in 0..3 {
        replica.sync_enqueue_missing_state_cells(&cells).unwrap();
        let publications = replica.sync_publishable_mutations().unwrap();
        assert_eq!(publications.len(), 1);
        assert_eq!((sync_common::transport::representation::<_, sync_common::OpaquePublication>(&(publications[0])).unwrap()).state.version.clone(), expected);
        let stored: (u64, String, u64) = replica
            .connection
            .query_row("SELECT changed_at,replica_id,replica_seq FROM sync_state_version WHERE state_kind='metadata'", [], |r| Ok((r.get::<_, i64>(0)? as u64, r.get(1)?, r.get::<_, i64>(2)? as u64)))
            .unwrap();
        assert_eq!(stored, (expected.changed_at, expected.replica_id.to_string(), expected.replica_seq.get()), "publication must not change local merge state");
        let reopened = Database::open(&path).unwrap();
        reopened.initialize_library().unwrap();
        assert_eq!(reopened.sync_publishable_mutations().unwrap(), publications, "origin is part of the durable retry snapshot");
        replica.sync_acknowledge_mutations(&[publications[0].publisher_seq]).unwrap();
    }
    drop(replica);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn import_retry_accepts_cycle_repair_but_rejects_a_real_rename() {
    let replica = db();
    let parent = uuid::Uuid::from_u128(1);
    let child = uuid::Uuid::from_u128(2);
    replica.create_directory_with_id(&parent, &ROOT_DIR_ID, &"Parent".into()).unwrap();
    replica.create_directory_with_id(&child, &parent, &"Child".into()).unwrap();
    let time = replica.sync_publishable_mutations().unwrap().iter().map(|value| value.state.version.changed_at).max().unwrap() + 1;
    pull(&replica, &[change(LibraryStateEntry::DirectoryParent { dir_id: parent, parent_id: child }, time, 20)]);
    assert_eq!(replica.directory_relative_path_string(&child).unwrap(), Some("Child".into()));
    replica.create_directory_with_id(&child, &parent, &"Child".into()).unwrap();
    replica.move_directory(&child, None, Some("Renamed")).unwrap();
    assert!(replica.create_directory_with_id(&child, &parent, &"Child".into()).is_err());
}

/// Model a server page for fixtures whose books already exist on the server:
/// a referenced book without a lifecycle in the page gets a Present one, and
/// lifecycles precede fields. Tests of absent-book behavior intentionally call
/// sync_commit_pull_response directly instead of using this helper.
pub(super) fn existing_books_page(mut page: PullStateResponse) -> PullStateResponse {
    let decoded = |event: &StateDelivery| LibraryStateEntry::decode(&event.state).ok().flatten();
    let mut books = page.mutations.iter().filter_map(decoded).filter(|body| matches!(body, LibraryStateEntry::BookLifecycle { .. })).filter_map(|body| apply::registers::book_key(&body).map(str::to_owned)).collect::<std::collections::HashSet<_>>();
    let mut lifecycles = Vec::new();
    for body in page.mutations.iter().filter_map(decoded) {
        let Some(key) = apply::registers::book_key(&body) else { continue };
        if books.insert(key.to_owned()) {
            lifecycles.push(change(LibraryStateEntry::BookLifecycle { content_hash: ContentHash::new(key), value: BookLifecycleState::Present }, 0, 1));
        }
    }
    let (existing, fields): (Vec<_>, Vec<_>) = page.mutations.into_iter().partition(|event| event.state.value.address.kind == "book_lifecycle");
    page.mutations = lifecycles.into_iter().chain(existing).chain(fields).collect();
    page
}

impl Database {
    pub(crate) fn commit_existing_books_fixture(&self, ids: &[ReplicaSeq], page: &PullStateResponse) -> Result<bool, crate::DatabaseError> {
        self.sync_commit_pull_response(ids, &existing_books_page(page.clone()))
    }
}

/// Model server acknowledgement only after proving these publications already
/// match its winning fields. Reconciliation must not invent a new edit/version.
pub(super) fn acknowledge_confirmed_reconciliation(replica: &Database, server: &[StateDelivery]) {
    let pending = replica.sync_publishable_mutations().unwrap();
    for publication in &pending {
        let wire = sync_common::transport::representation::<_, sync_common::OpaquePublication>(&(publication)).unwrap();
        assert!(wire.state.value.book.is_some() && wire.state.value.address.kind != "book_lifecycle", "reconciliation must only publish owned fields");
        let winner = server
            .iter()
            .filter(|event| {
                (event.state.value.address.kind.as_str(), event.state.value.address.entity_key.as_str(), event.state.value.address.entity_subkey.as_str())
                    == (wire.state.value.address.kind.as_str(), wire.state.value.address.entity_key.as_str(), wire.state.value.address.entity_subkey.as_str())
            })
            .max_by_key(|event| event.state.version.clone())
            .expect("publication must exist in the server fixture");
        assert_eq!(wire.state.value.bytes, winner.state.value.bytes);
        assert_eq!((wire).state.version.clone(), winner.state.version.clone());
    }
    replica.sync_acknowledge_mutations(&pending.iter().map(|m| m.publisher_seq).collect::<Vec<_>>()).unwrap();
}
