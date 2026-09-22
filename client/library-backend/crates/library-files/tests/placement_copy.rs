#![cfg(not(target_arch = "wasm32"))]

use library_database::Database;
use library_files::AssetStore;
use sync_common::ROOT_DIR_ID;

#[tokio::test]
async fn replay_retires_completed_generations_but_keeps_newer_and_blocked_work() {
    let root = tempfile::tempdir().unwrap();
    let db = Database::open(root.path().join("library.db")).unwrap();
    db.initialize_library().unwrap();
    let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    let dir = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    let hash = sync_common::ContentHash::new(&"a".repeat(64));
    db.queue_book_work(&hash).unwrap();
    let old_dirs: Vec<_> = db.native_directory_replay_work().unwrap().into_iter().map(|(id, _)| id).collect();
    let old_books = db.native_book_replay_work_ids().unwrap();
    db.queue_directory_work(&dir).unwrap();
    db.queue_book_work(&hash).unwrap();
    db.complete_native_directory_replay(&old_dirs).unwrap();
    db.complete_native_book_replay(&old_books).unwrap();
    assert_eq!(db.native_directory_work_ids().unwrap(), vec![dir]);
    assert_eq!(db.local_book_work_hashes().unwrap(), vec![hash]);
    store.run_directory_work(&db).await.unwrap();
    db.complete_native_book_replay(&old_books).unwrap();
    assert_eq!(db.local_book_work_hashes().unwrap(), vec![hash], "old completion must not retire a newer generation");
    store.run_file_jobs(&db).await.unwrap();
    assert!(root.path().join("Shelf").is_dir());
    assert!(db.native_directory_work_ids().unwrap().is_empty());
    assert!(db.local_book_work_hashes().unwrap().is_empty());

    // Skipping a leased file is not successful book reconciliation.
    db.execute("INSERT INTO book(content_hash,title,format) VALUES(?1,'book','epub')", [hash.as_str()]).unwrap();
    db.seed_file_work("trash", &hash, "/missing.epub");
    db.queue_book_work(&hash).unwrap();
    let lease = store.lease_book(&hash).await.unwrap();
    store.run_file_jobs(&db).await.unwrap();
    assert_eq!(db.local_book_work_hashes().unwrap(), vec![hash]);
    drop(lease);
    store.run_file_jobs(&db).await.unwrap();
    assert!(db.native_file_work().unwrap().is_empty());
    assert!(db.local_book_work_hashes().unwrap().is_empty());
}

#[tokio::test]
async fn failed_directory_replay_keeps_its_generation() {
    let root = tempfile::tempdir().unwrap();
    let db = Database::open(root.path().join("library.db")).unwrap();
    db.initialize_library().unwrap();
    let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    let dir = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    store.run_directory_work(&db).await.unwrap();
    db.move_directory(&dir, None, Some("Renamed")).unwrap();
    // A foreign identity must fail replay without acknowledging queued work.
    std::fs::write(root.path().join("Shelf/.biblos_uuid"), "foreign identity").unwrap();
    let work = db.native_directory_replay_work().unwrap();
    assert!(store.run_directory_work(&db).await.is_err());
    assert_eq!(db.native_directory_replay_work().unwrap(), work);
}

#[tokio::test]
async fn copy_then_remove_source_preserves_destination_bytes() {
    let root = tempfile::tempdir().unwrap();
    let db = Database::open(root.path().join("library.db")).unwrap();
    db.initialize_library().unwrap();
    let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    let source = db.create_directory(&ROOT_DIR_ID, &"A".into()).unwrap().id;
    let destination = db.create_directory(&ROOT_DIR_ID, &"Z".into()).unwrap().id;
    for id in [source, destination] {
        store.placement().prepare_directory_with_database(&db, id).await.unwrap();
    }
    let bytes = b"book bytes for placement copy regression";
    let hash = book_identity::identify(&mut std::io::Cursor::new(bytes)).unwrap();
    std::fs::write(root.path().join("A/book.epub"), bytes).unwrap();
    db.execute("INSERT INTO book(content_hash,title,format) VALUES(?1,'book.epub','epub')", [hash.as_str()]).unwrap();
    db.restore_book_placement(&hash, &source).unwrap();
    db.seed_file_projection(&hash, &source, "/A/book.epub");
    store.placement().reconcile_file_work(&db).await.unwrap();
    db.transfer_book_placement(&hash, &source, &destination, false).unwrap();
    store.placement().reconcile_file_work(&db).await.unwrap();
    assert_eq!(std::fs::read(root.path().join("A/book.epub")).unwrap(), bytes);
    assert_eq!(std::fs::read(root.path().join("Z/book.epub")).unwrap(), bytes);
    db.remove_book_placement(&hash, &source).unwrap();
    store.placement().reconcile_file_work(&db).await.unwrap();
    assert!(!root.path().join("A/book.epub").exists());
    assert_eq!(std::fs::read(root.path().join("Z/book.epub")).unwrap(), bytes);
    assert!(db.native_file_work().unwrap().is_empty());
}

#[tokio::test]
async fn cycle_recovery_converges_from_opposite_physical_moves() {
    use library_replica::{MutationBody, StateMutation};
    use sync_common::{LibraryRevision, MutationId, PullStateResponse, ReplicaSeq, ServerMutation, SyncCursor};
    let a = "00000000-0000-0000-0000-000000000001".parse().unwrap();
    let b = "00000000-0000-0000-0000-000000000002".parse().unwrap();
    for reverse in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let db = Database::open(root.path().join("library.db")).unwrap();
        db.initialize_library().unwrap();
        let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
        db.create_directory_with_id(&a, &ROOT_DIR_ID, &"A".into()).unwrap();
        db.create_directory_with_id(&b, &ROOT_DIR_ID, &"B".into()).unwrap();
        store.run_directory_work(&db).await.unwrap();
        std::fs::write(root.path().join("A/book.epub"), b"A bytes").unwrap();
        std::fs::write(root.path().join("B/book.epub"), b"B bytes").unwrap();
        for (dir, digit) in [(a, 'a'), (b, 'b')] {
            let hash = sync_common::ContentHash::new(&digit.to_string().repeat(64));
            db.execute("INSERT INTO book(content_hash,title,format) VALUES(?1,'book','epub')", [hash.as_str()]).unwrap();
            db.add_book_placement(&dir, &hash, "book.epub", "", true);
        }
        // Fail promptly if an intermediate cycle makes a recursive trigger loop.
        let steps = std::sync::atomic::AtomicUsize::new(0);
        db.progress_handler(1000, Some(move || steps.fetch_add(1, std::sync::atomic::Ordering::Relaxed) > 10_000)).unwrap();
        if reverse {
            db.move_directory(&b, Some(&a), None).unwrap();
        } else {
            db.move_directory(&a, Some(&b), None).unwrap();
        }
        store.run_directory_work(&db).await.unwrap();
        let replica = "00000000-0000-0000-0000-000000000003".parse().unwrap();
        // Both clients receive identical winning parents, in opposite orders.
        let mut mutations: Vec<_> = [(a, b), (b, a)].into_iter().enumerate().map(|(index, (dir_id, parent_id))| {
            let mutation = StateMutation {
                mutation_id: MutationId::parse(&format!("00000000-0000-0000-0000-{:012}", index + 10)).unwrap(),
                body: MutationBody::DirectoryParent { dir_id, parent_id },
                changed_at: 4_000_000_000_000,
                replica_seq: ReplicaSeq::new(index as u64 + 1).unwrap(),
            };
            ServerMutation { mutation: mutation.to_wire().unwrap(), replica_id: replica, revision: LibraryRevision::new(index as u64 + 1).unwrap() }
        }).collect();
        if reverse { mutations.reverse(); }
        let ids = db.sync_publishable_mutations().unwrap().iter().map(|m| m.mutation_id).collect::<Vec<_>>();
        let page = PullStateResponse { mutations, next_cursor: SyncCursor { state_revision: LibraryRevision::new(2).ok(), reading_revision: None }, has_more: false };
        db.sync_commit_pull_response(&ids, &page).unwrap();
        assert_eq!(db.directory_relative_path_string(&b).unwrap().as_deref(), Some("B"));
        assert_eq!(db.directory_relative_path_string(&a).unwrap().as_deref(), Some("B/A"));
        assert!(db.sync_publishable_mutations().unwrap().is_empty(), "repair must not emit a new move");
        store.run_directory_work(&db).await.unwrap();
        assert_eq!(std::fs::read(root.path().join("B/A/book.epub")).unwrap(), b"A bytes");
        assert_eq!(std::fs::read(root.path().join("B/book.epub")).unwrap(), b"B bytes");
        assert!(!root.path().join("A").exists());
        assert!(db.native_directory_work_ids().unwrap().is_empty());
        db.sync_commit_pull_response(&[], &page).unwrap();
        store.run_directory_work(&db).await.unwrap();
        assert_eq!(std::fs::read(root.path().join("B/A/book.epub")).unwrap(), b"A bytes");
    }
}

#[tokio::test]
async fn managed_name_swap_preserves_resolved_names_and_files() {
    let root = tempfile::tempdir().unwrap();
    let db = Database::open(root.path().join("library.db")).unwrap();
    db.initialize_library().unwrap();
    let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    let a = db.create_directory(&ROOT_DIR_ID, &"Shelf".into()).unwrap().id;
    let b = db.create_directory(&ROOT_DIR_ID, &"Shelf 2".into()).unwrap().id;
    store.run_directory_work(&db).await.unwrap();
    std::fs::write(root.path().join("Shelf/a.epub"), b"A").unwrap();
    std::fs::write(root.path().join("Shelf 2/b.epub"), b"B").unwrap();
    // Equivalent to a resolver exchanging the unnumbered winner and loser.
    db.execute("UPDATE sync_metadata SET change_origin='remote'", []).unwrap();
    db.execute("UPDATE dir SET deleted_at=1 WHERE id IN (?1,?2)", [a.to_string(), b.to_string()]).unwrap();
    db.execute("UPDATE dir SET name='Shelf 2' WHERE id=?1", [a.to_string()]).unwrap();
    db.execute("UPDATE dir SET name='Shelf' WHERE id=?1", [b.to_string()]).unwrap();
    db.execute("UPDATE dir SET deleted_at=NULL WHERE id IN (?1,?2)", [a.to_string(), b.to_string()]).unwrap();
    db.execute("UPDATE sync_metadata SET change_origin='local'", []).unwrap();
    db.queue_directory_work(&a).unwrap();
    db.queue_directory_work(&b).unwrap();
    let before = db.sync_publishable_mutations().unwrap().iter().map(|m| m.mutation_id).collect::<Vec<_>>();
    store.placement().prepare_directory_with_database(&db, a).await.unwrap();
    assert_eq!(std::fs::read(root.path().join("Shelf 2/a.epub")).unwrap(), b"A");
    assert_eq!(std::fs::read(root.path().join("Shelf/b.epub")).unwrap(), b"B");
    assert_eq!(db.directory_relative_path_string(&a).unwrap().as_deref(), Some("Shelf 2"));
    assert_eq!(db.directory_relative_path_string(&b).unwrap().as_deref(), Some("Shelf"));
    assert_eq!(before, db.sync_publishable_mutations().unwrap().iter().map(|m| m.mutation_id).collect::<Vec<_>>());
    assert!(!std::fs::read_dir(root.path()).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().starts_with(".biblos-move-")));
}

#[tokio::test]
async fn upgrade_recovers_suppressed_cycle_without_outbox_changes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("library.db");
    let a = "00000000-0000-0000-0000-000000000001".parse().unwrap();
    let b = "00000000-0000-0000-0000-000000000002".parse().unwrap();
    let db = Database::open(&path).unwrap();
    db.initialize_library().unwrap();
    db.create_directory_with_id(&a, &ROOT_DIR_ID, &"A".into()).unwrap();
    db.create_directory_with_id(&b, &ROOT_DIR_ID, &"B".into()).unwrap();
    let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    store.run_directory_work(&db).await.unwrap();
    std::fs::write(root.path().join("A/book.epub"), b"retained").unwrap();
    db.execute("UPDATE sync_metadata SET change_origin='remote'", []).unwrap();
    db.execute("UPDATE dir SET intent_parent_id=?2,deleted_at=1 WHERE id=?1", [a.to_string(), b.to_string()]).unwrap();
    db.execute("UPDATE dir SET intent_parent_id=?2,deleted_at=1 WHERE id=?1", [b.to_string(), a.to_string()]).unwrap();
    db.execute("UPDATE sync_metadata SET change_origin='local'", []).unwrap();
    let ids = db.sync_publishable_mutations().unwrap().iter().map(|m| m.mutation_id).collect::<Vec<_>>();
    db.sync_acknowledge_mutations(&ids).unwrap();
    db.pragma_update(None, "user_version", 67).unwrap();
    drop(db);
    let db = Database::open(&path).unwrap();
    db.initialize_library().unwrap();
    assert_eq!(db.directory_relative_path_string(&a).unwrap().as_deref(), Some("B/A"));
    assert_eq!(db.directory_relative_path_string(&b).unwrap().as_deref(), Some("B"));
    assert!(db.sync_publishable_mutations().unwrap().is_empty());
    assert!(!db.native_directory_work_ids().unwrap().is_empty());
    store.run_directory_work(&db).await.unwrap();
    assert_eq!(std::fs::read(root.path().join("B/A/book.epub")).unwrap(), b"retained");
    db.initialize_library().unwrap();
    assert!(db.native_directory_work_ids().unwrap().is_empty());
    assert!(db.sync_publishable_mutations().unwrap().is_empty());
}

#[tokio::test]
async fn directory_replay_recovers_unacknowledged_staging_with_descendants() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("library.db");
    let db = Database::open(&path).unwrap();
    db.initialize_library().unwrap();
    let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    let parent = db.create_directory(&ROOT_DIR_ID, &"Old".into()).unwrap().id;
    let child = db.create_directory(&parent, &"Child".into()).unwrap().id;
    store.run_directory_work(&db).await.unwrap();
    std::fs::write(root.path().join("Old/Child/book.epub"), b"retained").unwrap();
    db.move_directory(&parent, None, Some("New")).unwrap();
    // Crash window: filesystem rename completed, projection acknowledgement did not.
    std::fs::rename(root.path().join("Old"), root.path().join(format!(".biblos-move-{parent}"))).unwrap();
    drop(db);
    let db = Database::open(&path).unwrap();
    db.initialize_library().unwrap();
    store.run_directory_work(&db).await.unwrap();
    assert_eq!(std::fs::read(root.path().join("New/Child/book.epub")).unwrap(), b"retained");
    assert_eq!(db.native_directory_snapshot(&child).unwrap().unwrap().projected_path.as_deref(), Some("/New/Child"));
    assert!(db.native_directory_work_ids().unwrap().is_empty());
}

#[tokio::test]
async fn interrupted_staging_is_recovered_after_rename_undo() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("library.db");
    let db = Database::open(&path).unwrap();
    db.initialize_library().unwrap();
    let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    let id = db.create_directory(&ROOT_DIR_ID, &"Old".into()).unwrap().id;
    store.run_directory_work(&db).await.unwrap();
    std::fs::write(root.path().join("Old/book.epub"), b"book").unwrap();
    db.move_directory(&id, None, Some("New")).unwrap();
    let staged = root.path().join(format!(".biblos-move-{id}"));
    std::fs::rename(root.path().join("Old"), &staged).unwrap();
    drop(db);
    let db = Database::open(&path).unwrap();
    db.initialize_library().unwrap();
    db.move_directory(&id, None, Some("Old")).unwrap();
    store.run_directory_work(&db).await.unwrap();
    assert_eq!(std::fs::read(root.path().join("Old/book.epub")).unwrap(), b"book");
    assert!(!staged.exists());
    assert!(db.native_directory_work_ids().unwrap().is_empty());
}

#[test]
fn repaired_directory_rename_preserves_parent_intent_and_explicit_move_publishes_it() {
    let root = tempfile::tempdir().unwrap();
    let db = Database::open(root.path().join("library.db")).unwrap();
    db.initialize_library().unwrap();
    let a = "00000000-0000-0000-0000-000000000001".parse().unwrap();
    let b = "00000000-0000-0000-0000-000000000002".parse().unwrap();
    db.create_directory_with_id(&a, &ROOT_DIR_ID, &"A".into()).unwrap();
    db.create_directory_with_id(&b, &ROOT_DIR_ID, &"B".into()).unwrap();
    db.execute("UPDATE dir SET intent_parent_id=?2 WHERE id=?1", [a.to_string(), b.to_string()]).unwrap();
    db.execute("UPDATE dir SET intent_parent_id=?2 WHERE id=?1", [b.to_string(), a.to_string()]).unwrap();
    db.pragma_update(None, "user_version", 67).unwrap();
    db.initialize_library().unwrap();
    let ids = db.sync_publishable_mutations().unwrap().iter().map(|m| m.mutation_id).collect::<Vec<_>>();
    db.sync_acknowledge_mutations(&ids).unwrap();
    db.move_directory(&b, None, Some("Renamed B")).unwrap();
    let parent = || db.query_row("SELECT intent_parent_id FROM dir WHERE id=?1", [b.to_string()], |r| r.get::<_, String>(0)).unwrap();
    assert_eq!(parent(), a.to_string());
    let changes = db.sync_publishable_mutations().unwrap();
    assert_eq!(changes.len(), 1);
    assert!(matches!(&changes[0].body, library_replica::MutationBody::DirectoryName { dir_id, name } if *dir_id == b && name == "Renamed B"));
    db.sync_acknowledge_mutations(&[changes[0].mutation_id]).unwrap();
    // Explicitly moving to the already-displayed root clears the cyclic intent.
    db.move_directory(&b, Some(&ROOT_DIR_ID), None).unwrap();
    assert_eq!(parent(), ROOT_DIR_ID.to_string());
    let changes = db.sync_publishable_mutations().unwrap();
    assert_eq!(changes.len(), 1);
    assert!(matches!(&changes[0].body, library_replica::MutationBody::DirectoryParent { dir_id, parent_id } if *dir_id == b && *parent_id == ROOT_DIR_ID));
    assert_eq!(db.directory_relative_path_string(&a).unwrap().as_deref(), Some("Renamed B/A"));
}

#[tokio::test]
async fn direct_preparation_recovers_interrupted_staging_before_creating_destination() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("library.db");
    let db = Database::open(&path).unwrap();
    db.initialize_library().unwrap();
    let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    let id = db.create_directory(&ROOT_DIR_ID, &"Old".into()).unwrap().id;
    store.run_directory_work(&db).await.unwrap();
    std::fs::write(root.path().join("Old/book.epub"), b"book").unwrap();
    db.move_directory(&id, None, Some("New")).unwrap();
    let staged = root.path().join(format!(".biblos-move-{id}"));
    std::fs::rename(root.path().join("Old"), &staged).unwrap();
    drop(db);
    let db = Database::open(&path).unwrap();
    db.initialize_library().unwrap();
    let before = db.sync_publishable_mutations().unwrap().iter().map(|m| m.mutation_id).collect::<Vec<_>>();
    store.placement().prepare_directory_with_database(&db, id).await.unwrap();
    assert_eq!(std::fs::read(root.path().join("New/book.epub")).unwrap(), b"book");
    assert!(!staged.exists());
    assert_eq!(db.native_directory_snapshot(&id).unwrap().unwrap().projected_path.as_deref(), Some("/New"));
    assert!(db.native_directory_work_ids().unwrap().is_empty());
    store.run_directory_work(&db).await.unwrap();
    assert_eq!(before, db.sync_publishable_mutations().unwrap().iter().map(|m| m.mutation_id).collect::<Vec<_>>());
}

#[tokio::test]
async fn trash_after_interrupted_stage_preserves_bytes_in_trash() {
    let root = tempfile::tempdir().unwrap();
    let db = Database::open(root.path().join("library.db")).unwrap();
    db.initialize_library().unwrap();
    let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    let id = db.create_directory(&ROOT_DIR_ID, &"Old".into()).unwrap().id;
    store.run_directory_work(&db).await.unwrap();
    let bytes = b"book";
    let hash = book_identity::identify(&mut std::io::Cursor::new(bytes)).unwrap();
    std::fs::write(root.path().join("Old/book.epub"), bytes).unwrap();
    db.execute("INSERT INTO book(content_hash,title,format) VALUES(?1,'book','epub')", [hash.as_str()]).unwrap();
    db.add_book_placement(&id, &hash, "book.epub", "", true);
    db.seed_file_projection(&hash, &id, "/Old/book.epub");
    db.move_directory(&id, None, Some("New")).unwrap();
    let staged = root.path().join(format!(".biblos-move-{id}"));
    std::fs::rename(root.path().join("Old"), &staged).unwrap();
    db.trash_directory(&id).unwrap();
    store.run_file_jobs(&db).await.unwrap();
    assert!(!staged.join("book.epub").exists());
    assert!(db.native_directory_work_ids().unwrap().is_empty());
    assert!(db.native_file_work().unwrap().is_empty());
    // Restoring the book proves its bytes reached recoverable storage.
    db.restore_book_commit(&hash, &[(id, ROOT_DIR_ID)]).unwrap();
    store.run_file_jobs(&db).await.unwrap();
    assert_eq!(std::fs::read(root.path().join("book.epub")).unwrap(), bytes);
}

#[tokio::test]
async fn unrelated_broken_move_does_not_block_direct_preparation() {
    let root = tempfile::tempdir().unwrap();
    let db = Database::open(root.path().join("library.db")).unwrap();
    db.initialize_library().unwrap();
    let store = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    let broken = db.create_directory(&ROOT_DIR_ID, &"Broken".into()).unwrap().id;
    let good = db.create_directory(&ROOT_DIR_ID, &"Good".into()).unwrap().id;
    store.run_directory_work(&db).await.unwrap();
    db.move_directory(&broken, None, Some("Renamed")).unwrap();
    std::fs::write(root.path().join("Broken/.biblos_uuid"), "foreign").unwrap();
    store.placement().prepare_directory_with_database(&db, good).await.unwrap();
    assert_eq!(db.native_directory_work_ids().unwrap(), vec![broken]);
    assert!(root.path().join("Good/.biblos_uuid").is_file());
}
