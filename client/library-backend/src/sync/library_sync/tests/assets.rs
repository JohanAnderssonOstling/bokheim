use super::*;

/// Whether any device-local reconciliation is still queued: a directory move
/// awaiting physical materialization, a book awaiting follow-up file work,
/// or a file awaiting a trash/restore operation.
fn has_pending_file_work(database: &library_database::Database) -> bool {
    !database.native_directory_work_ids().unwrap().is_empty() || !database.native_book_work_directory_ids().unwrap().is_empty() || !database.native_file_work().unwrap().is_empty()
}

#[test]
fn only_new_path_changes_request_file_reconciliation() {
    let root = tempfile::tempdir().unwrap();
    let manager = test_manager(root.path());
    let database = &manager.database;
    let position = pulled_change(1, reading_change(1, 1, "epubcfi(/6/2)", 10).body);
    let cursor = database.sync_pull_cursor().unwrap();
    database.sync_commit_pull_response(&[], &sync_common::PullStateResponse { mutations: vec![position], next_cursor: cursor, has_more: false }).unwrap();
    for body in
        [MutationBody::DirectoryParent { dir_id: fixture_dir_id("moved"), parent_id: crate::ROOT_DIR_ID }, MutationBody::DirectoryLifecycle { dir_id: fixture_dir_id("moved"), value: library_replica::DirectoryLifecycleState::Present }]
    {
        let cursor = database.sync_pull_cursor().unwrap();
        database.sync_commit_pull_response(&[], &sync_common::PullStateResponse { mutations: vec![pulled_change(2, body)], next_cursor: cursor, has_more: false }).unwrap();
    }
    let moved = pulled_change(2, MutationBody::DirectoryName { dir_id: fixture_dir_id("moved"), name: "Shelf".into() });
    let cursor = database.sync_pull_cursor().unwrap();
    database.sync_commit_pull_response(&[], &sync_common::PullStateResponse { mutations: vec![moved.clone()], next_cursor: cursor, has_more: false }).unwrap();
    assert_eq!(database.native_directory_work_ids().unwrap(), vec![fixture_dir_id("moved")]);
    database.clear_local_directory_work();
    let cursor = database.sync_pull_cursor().unwrap();
    database.sync_commit_pull_response(&[], &sync_common::PullStateResponse { mutations: vec![moved], next_cursor: cursor, has_more: false }).unwrap();
    assert!(!has_pending_file_work(database), "an already applied change must not request another walk");
    let hash = fixture_content_hash(97);
    let placement = pulled_change(3, MutationBody::Placement { dir_id: crate::ROOT_DIR_ID, content_hash: hash, present: true, origin_folder_id: None });
    let cursor = database.sync_pull_cursor().unwrap();
    database.sync_commit_pull_response(&[], &sync_common::PullStateResponse { mutations: vec![placement], next_cursor: cursor, has_more: false }).unwrap();
    assert_eq!(database.local_book_work_hashes().unwrap(), vec![hash]);
    assert_eq!(database.native_directory_work_ids().unwrap().len(), 0, "a root placement must not reconcile the whole library");
}

#[test]
fn local_coverless_books_do_not_probe_remote_thumbnails() {
    assert!(!should_download_thumbnail(true, false, false, false));
    assert!(should_download_thumbnail(true, false, true, false));
    assert!(should_download_thumbnail(false, false, false, false));
    assert!(!should_download_thumbnail(false, true, true, false));
    assert!(!should_download_thumbnail(false, false, true, true));
}

#[tokio::test]
async fn asset_planning_leaves_moves_and_purges_to_local_jobs() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let mut manager = test_manager(temp.path());
    manager.credentials = test_shared_credentials(&manager.server_url);
    let bytes = b"local book";
    let hash = ContentHash::new(blake3::hash(bytes).to_hex().as_str());
    std::fs::write(temp.path().join("original.epub"), bytes).unwrap();
    {
        let conn = &manager.database;
        conn.seed_book(&hash, None, 1, "epub");
        conn.add_book_placement(&crate::ROOT_DIR_ID, &hash, "renamed.epub", "fp", true);
        conn.seed_file_projection(&hash, &crate::ROOT_DIR_ID, "/original.epub");
        // The live placement (book_dir, above) already disagrees with this
        // remembered physical path; queue the same restore/trash pair a real
        // placement mutation would through `queue_file_work`.
        conn.seed_file_work("restore", &hash, "/renamed.epub");
        conn.seed_file_work("trash", &hash, "/original.epub");
        conn.record_remote_asset(BlobKind::Book, &hash).unwrap();
        conn.complete_thumbnail_work(&hash, false).unwrap();
    }
    manager.request_download(hash).unwrap();
    manager.reconcile_assets(Vec::new()).await.unwrap();
    assert!(temp.path().join("original.epub").exists());
    assert!(!temp.path().join("renamed.epub").exists());
    assert!(has_pending_file_work(&manager.database));
    assert!(!manager.database.is_book_download_requested(&hash).unwrap(), "remembered source satisfies downloads before the rename runs");
    assert!(manager.transfer_statuses().is_empty(), "a pending rename needs no network transfer");

    manager.assets.run_file_jobs(&manager.database).await.unwrap();
    assert!(temp.path().join("renamed.epub").exists());
    assert!(!temp.path().join("original.epub").exists());
    manager.assets.write(BlobKind::Thumbnail, &hash, b"retained cover").await.unwrap();
    {
        let conn = &manager.database;
        conn.mark_all_placements_deleted();
        // Trash the book without purging it yet: purge_book_snapshot only
        // removes bytes for a not-yet-retired (purged_at still NULL) book,
        // treating an already-purged one as already cleaned up.
        conn.execute("UPDATE book SET deleted_at=1", []).unwrap();
        // A real deletion queues the placement's physical removal alongside
        // the durable Trash/purge state; this fixture bypasses trash_directory
        // and purge_book, so it must queue that file work itself.
        conn.seed_file_work("trash", &hash, "/renamed.epub");
        conn.seed_purge_work(&hash);
    }
    manager.reconcile_assets(Vec::new()).await.unwrap();
    assert!(manager.transfer_statuses().is_empty(), "deleted placements cannot become transfer candidates");
    assert!(temp.path().join("renamed.epub").exists());
    assert!(manager.assets.exists(BlobKind::Thumbnail, &hash).unwrap());
    manager.assets.run_file_jobs(&manager.database).await.unwrap();
    assert!(!temp.path().join("renamed.epub").exists());
    assert!(!manager.assets.exists(BlobKind::Thumbnail, &hash).unwrap());
    assert!(!manager.database.has_pending_purge_work().unwrap());
}

#[tokio::test]
async fn download_request_pages_complete_existing_files_without_transfers() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let mut manager = test_manager(temp.path());
    manager.credentials = test_shared_credentials(&manager.server_url);
    let mut hashes = Vec::new();
    for index in 0..65 {
        let bytes = format!("book {index}");
        let hash = ContentHash::new(blake3::hash(bytes.as_bytes()).to_hex().as_str());
        let name = format!("{index}.epub");
        std::fs::write(temp.path().join(&name), bytes).unwrap();
        let conn = &manager.database;
        conn.seed_book(&hash, None, 1, "epub");
        conn.add_book_placement(&crate::ROOT_DIR_ID, &hash, &name, "fp", false);
        conn.record_remote_asset(BlobKind::Book, &hash).unwrap();
        conn.complete_thumbnail_work(&hash, false).unwrap();
        hashes.push(hash);
    }
    manager.request_downloads(&hashes).unwrap();
    manager.reconcile_assets(Vec::new()).await.unwrap();
    let conn = &manager.database;
    assert!(conn.asset_requests(BlobKind::Book).unwrap().is_empty());
    for hash in hashes {
        assert!(conn.is_book_downloaded(&hash).unwrap());
    }
    assert!(manager.transfer_statuses().is_empty());
}

#[tokio::test]
async fn scheduled_download_completes_local_requests_without_network_io() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let mut manager = test_manager(temp.path());
    manager.credentials = test_shared_credentials(&manager.server_url);
    let hash = test_content_hash();
    {
        let conn = &manager.database;
        conn.seed_book(&hash, None, 1, "epub");
        conn.add_book_placement(&crate::ROOT_DIR_ID, &hash, "book.epub", "fp", false);
        conn.record_remote_asset(BlobKind::Book, &hash).unwrap();
        conn.shared_request_thumbnail(&hash).unwrap();
    }
    std::fs::write(temp.path().join("book.epub"), b"already downloaded").unwrap();
    manager.database.shared_set_book_hash_downloaded(&hash, false).unwrap();
    manager.request_download(hash).unwrap();
    // Asset scheduling remains independent of metadata synchronization.
    let _gate = manager.sync_gate.lock().await;
    tokio::time::timeout(std::time::Duration::from_secs(3), manager.sync_assets(Vec::new())).await.unwrap().unwrap();
    assert!(!manager.database.is_book_download_requested(&hash).unwrap());
    assert!(manager.database.is_book_downloaded(&hash).unwrap());
}

#[test]
fn requested_download_uses_content_hash_identity() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let manager = test_manager(temp.path());
    let conn = &manager.database;
    let dir_id = fixture_dir_id("d1");
    conn.seed_dir(&dir_id, &crate::ROOT_DIR_ID, "Shelf");
    conn.seed_book(&test_content_hash(), None, 1, "epub");
    conn.add_book_placement(&dir_id, &test_content_hash(), "book.epub", "fp", false);

    let job = manager.prepare_requested_download(&test_content_hash()).unwrap().expect("live placement should produce a download job");
    assert_eq!(job, TransferJob::download_blob(test_content_hash(), db_placement(&crate::BookPlacement::new(test_content_hash(), crate::RelativeBookPath::parse("/Shelf/book.epub").unwrap())), TransferOrigin::UserInitiated,));
}

#[test]
fn download_request_requires_and_resolves_a_live_placement() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let manager = test_manager(temp.path());
    {
        let conn = &manager.database;
        conn.seed_book(&test_content_hash(), None, 1, "epub");
    }

    assert!(manager.request_download(test_content_hash()).is_err());

    {
        let conn = &manager.database;
        conn.add_book_placement(&crate::ROOT_DIR_ID, &test_content_hash(), "book.epub", "fp", false);
    }
    manager.request_download(test_content_hash()).unwrap();

    let conn = &manager.database;
    assert_eq!(conn.asset_requests(BlobKind::Book).unwrap(), vec![(test_content_hash(), TransferOrigin::UserInitiated)]);
}

#[test]
fn durable_download_intent_rederives_work_after_queue_recreation() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let manager = test_manager(temp.path());
    {
        let conn = &manager.database;
        conn.seed_book(&test_content_hash(), None, 1, "epub");
        conn.add_book_placement(&crate::ROOT_DIR_ID, &test_content_hash(), "book.epub", "fp", false);
    }
    manager.request_download(test_content_hash()).unwrap();
    assert_eq!(manager.transfer_queue.statuses()[0].total_items, 1, "the live scheduler receives the request immediately");
    drop(manager);

    let restored = test_manager(temp.path());
    let conn = &restored.database;
    assert_eq!(conn.asset_requests(BlobKind::Book).unwrap(), vec![(test_content_hash(), TransferOrigin::UserInitiated)]);
    assert!(restored.prepare_requested_download(&test_content_hash()).unwrap().is_some());
    assert!(restored.transfer_queue.statuses().is_empty());
}
