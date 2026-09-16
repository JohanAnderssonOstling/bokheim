use super::*;

#[test]
fn only_new_path_changes_request_file_reconciliation() {
    let root = tempfile::tempdir().unwrap();
    let manager = test_manager(root.path());
    let mut conn = manager.database.get().unwrap();
    let position = pulled_change(1, reading_change(1, 1, "epubcfi(/6/2)", 10).body);
    let tx = conn.transaction().unwrap();
    client_library::remote_reducer::apply_remote_change_in_tx(&tx, &position).unwrap();
    tx.commit().unwrap();
    assert!(client_library::sync_state::get_sync_metadata(&conn, "local_file_recovery_pending").unwrap().is_none());
    for body in [MutationBody::DirectoryParent { dir_id: fixture_dir_id("moved"), parent_id: crate::ROOT_DIR_ID }, MutationBody::DirectoryLifecycle { dir_id: fixture_dir_id("moved"), value: library_model::DirectoryLifecycleState::Present }]
    {
        let tx = conn.transaction().unwrap();
        client_library::remote_reducer::apply_remote_change_in_tx(&tx, &pulled_change(2, body)).unwrap();
        tx.commit().unwrap();
    }
    let moved = pulled_change(2, MutationBody::DirectoryName { dir_id: fixture_dir_id("moved"), name: "Shelf".into() });
    let tx = conn.transaction().unwrap();
    client_library::remote_reducer::apply_remote_change_in_tx(&tx, &moved).unwrap();
    tx.commit().unwrap();
    assert_eq!(conn.query_row(include_str!("../../sql/library_sync/only_new_path_changes_request_file_reconciliation_select.sql"), [], |r| r.get::<_, String>(0)).unwrap(), fixture_dir_id("moved").to_string());
    conn.execute(include_str!("../../sql/library_sync/only_new_path_changes_request_file_reconciliation_delete.sql"), []).unwrap();
    let tx = conn.transaction().unwrap();
    client_library::remote_reducer::apply_remote_change_in_tx(&tx, &moved).unwrap();
    tx.commit().unwrap();
    assert!(!client_library::file_work::has_pending_file_work(&conn).unwrap(), "an already applied change must not request another walk");
    let hash = fixture_content_hash(97);
    let placement = pulled_change(3, MutationBody::Placement { dir_id: crate::ROOT_DIR_ID, content_hash: hash, present: true, origin_folder_id: None });
    let tx = conn.transaction().unwrap();
    client_library::remote_reducer::apply_remote_change_in_tx(&tx, &placement).unwrap();
    tx.commit().unwrap();
    assert_eq!(conn.query_row(include_str!("../../sql/library_sync/only_new_path_changes_request_file_reconciliation_select_2.sql"), [], |r| r.get::<_, String>(0)).unwrap(), hash.to_string());
    assert_eq!(conn.query_row(include_str!("../../sql/library_sync/only_new_path_changes_request_file_reconciliation_select_3.sql"), [], |r| r.get::<_, i64>(0)).unwrap(), 0, "a root placement must not reconcile the whole library");
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
    manager.credentials = Arc::new(std::sync::RwLock::new(Some(test_credentials(&manager.server_url))));
    let bytes = b"local publication";
    let hash = ContentHash::new(blake3::hash(bytes).to_hex().as_str());
    std::fs::write(temp.path().join("original.epub"), bytes).unwrap();
    {
        let conn = manager.database.get().unwrap();
        conn.execute(include_str!("../../sql/library_sync/asset_planning_leaves_moves_and_purges_to_local_jobs_insert.sql"), [hash.as_str()]).unwrap();
        conn.execute(include_str!("../../sql/library_sync/asset_planning_leaves_moves_and_purges_to_local_jobs_insert_2.sql"), [hash.as_str()]).unwrap();
        conn.execute(include_str!("../../sql/library_sync/asset_planning_leaves_moves_and_purges_to_local_jobs_insert_3.sql"), [hash.as_str()]).unwrap();
        client_library::file_work::queue_book_work(&conn, &hash).unwrap();
        client_library::assets::record_remote_asset(&conn, BlobKind::Book, &hash).unwrap();
        client_library::thumbnail_jobs::complete_thumbnail_work(&conn, &hash, false).unwrap();
    }
    manager.request_download(hash).unwrap();
    manager.reconcile_assets().await.unwrap();
    assert!(temp.path().join("original.epub").exists());
    assert!(!temp.path().join("renamed.epub").exists());
    assert!(client_library::file_work::has_pending_file_work(&manager.database.get().unwrap()).unwrap());
    assert!(!client_library::assets::is_book_download_requested(&manager.database.get().unwrap(), hash).unwrap(), "remembered source satisfies downloads before the rename runs");
    assert!(manager.transfer_statuses().is_empty(), "a pending rename needs no network transfer");

    manager.assets.run_file_jobs(&manager.database).await.unwrap();
    assert!(temp.path().join("renamed.epub").exists());
    assert!(!temp.path().join("original.epub").exists());
    manager.assets.write(BlobKind::Thumbnail, &hash, b"retained cover").await.unwrap();
    {
        let conn = manager.database.get().unwrap();
        conn.execute(include_str!("../../sql/library_sync/asset_planning_leaves_moves_and_purges_to_local_jobs_update.sql"), []).unwrap();
        conn.execute(include_str!("../../sql/library_sync/asset_planning_leaves_moves_and_purges_to_local_jobs_update_2.sql"), []).unwrap();
        client_library::file_work::queue_book_work(&conn, &hash).unwrap();
        conn.execute(include_str!("../../sql/library_sync/asset_planning_leaves_moves_and_purges_to_local_jobs_insert_4.sql"), [hash.as_str()]).unwrap();
    }
    manager.reconcile_assets().await.unwrap();
    assert!(manager.transfer_statuses().is_empty(), "deleted placements cannot become transfer candidates");
    assert!(temp.path().join("renamed.epub").exists());
    assert!(manager.assets.exists(BlobKind::Thumbnail, &hash).unwrap());
    manager.assets.run_file_jobs(&manager.database).await.unwrap();
    assert!(!temp.path().join("renamed.epub").exists());
    assert!(!manager.assets.exists(BlobKind::Thumbnail, &hash).unwrap());
    assert_eq!(manager.database.get().unwrap().query_row(include_str!("../../sql/library_sync/asset_planning_leaves_moves_and_purges_to_local_jobs_select.sql"), [], |row| row.get::<_, i64>(0)).unwrap(), 0);
}

#[tokio::test]
async fn download_request_pages_complete_existing_files_without_transfers() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let mut manager = test_manager(temp.path());
    manager.credentials = Arc::new(std::sync::RwLock::new(Some(test_credentials(&manager.server_url))));
    let mut hashes = Vec::new();
    for index in 0..65 {
        let bytes = format!("publication {index}");
        let hash = ContentHash::new(blake3::hash(bytes.as_bytes()).to_hex().as_str());
        let name = format!("{index}.epub");
        std::fs::write(temp.path().join(&name), bytes).unwrap();
        let conn = manager.database.get().unwrap();
        conn.execute(include_str!("../../sql/library_sync/asset_planning_leaves_moves_and_purges_to_local_jobs_insert.sql"), [hash.as_str()]).unwrap();
        conn.execute(include_str!("../../sql/asset_sync/queued_plan_reads_multiple_pages_without_consuming_requests_insert_2.sql"), rusqlite::params![hash.as_str(), name]).unwrap();
        client_library::assets::record_remote_asset(&conn, BlobKind::Book, &hash).unwrap();
        client_library::thumbnail_jobs::complete_thumbnail_work(&conn, &hash, false).unwrap();
        hashes.push(hash);
    }
    manager.request_downloads(&hashes).unwrap();
    manager.reconcile_assets().await.unwrap();
    let conn = manager.database.get().unwrap();
    assert!(client_library::assets::get_asset_requests(&conn, BlobKind::Book).unwrap().is_empty());
    for hash in hashes {
        assert!(client_library::assets::is_book_downloaded(&conn, hash).unwrap());
    }
    assert!(manager.transfer_statuses().is_empty());
}

#[tokio::test]
async fn all_download_paths_complete_local_requests_without_network_io() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let mut manager = test_manager(temp.path());
    manager.credentials = Arc::new(std::sync::RwLock::new(Some(test_credentials(&manager.server_url))));
    let hash = test_content_hash();
    {
        let conn = manager.database.get().unwrap();
        conn.execute(include_str!("../../sql/asset_sync/fixture_insert.sql"), [hash.as_str()]).unwrap();
        conn.execute(include_str!("../../sql/asset_sync/fixture_insert_2.sql"), [hash.as_str()]).unwrap();
        client_library::assets::record_remote_asset(&conn, BlobKind::Book, &hash).unwrap();
        client_library::thumbnail_jobs::request_thumbnail(&conn, &hash).unwrap();
    }
    std::fs::write(temp.path().join("book.epub"), b"already downloaded").unwrap();
    for route in 0..2 {
        client_library::placements::set_book_hash_downloaded(&manager.database.get().unwrap(), hash, false).unwrap();
        manager.request_download(hash).unwrap();
        // An immediate reader request must bypass the library-wide sync gate.
        let _gate = manager.sync_gate.lock().await;
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            match route {
                0 => manager.download_requested(hash).await,
                _ => manager.sync_assets().await,
            }
        })
        .await
        .unwrap()
        .unwrap();
        let conn = manager.database.get().unwrap();
        assert!(!client_library::assets::is_book_download_requested(&conn, hash).unwrap());
        assert!(client_library::assets::is_book_downloaded(&conn, hash).unwrap());
    }
}

#[test]
fn requested_download_uses_content_hash_identity() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let manager = test_manager(temp.path());
    let conn = manager.database.get().unwrap();
    let dir_id = fixture_dir_id("d1").to_string();
    conn.execute(include_str!("../../sql/library_sync/requested_download_uses_content_hash_identity_insert.sql"), [dir_id.as_str()]).unwrap();
    conn.execute(include_str!("../../sql/asset_sync/fixture_insert.sql"), [TEST_CONTENT_HASH]).unwrap();
    conn.execute(include_str!("../../sql/library_sync/requested_download_uses_content_hash_identity_insert_3.sql"), rusqlite::params![TEST_CONTENT_HASH, dir_id]).unwrap();

    let job = LibrarySync::prepare_requested_download(&manager.assets, &conn, &test_content_hash()).unwrap().expect("live placement should produce a download job");
    assert_eq!(job, TransferJob::download_blob(test_content_hash(), crate::BookPlacement::new(test_content_hash(), crate::RelativeBookPath::parse("/Shelf/book.epub").unwrap()), TransferOrigin::UserInitiated,));
}

#[test]
fn download_request_requires_and_resolves_a_live_placement() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let manager = test_manager(temp.path());
    {
        let conn = manager.database.get().unwrap();
        conn.execute(include_str!("../../sql/asset_sync/fixture_insert.sql"), [TEST_CONTENT_HASH]).unwrap();
    }

    assert!(manager.request_download(test_content_hash()).is_err());

    {
        let conn = manager.database.get().unwrap();
        conn.execute(include_str!("../../sql/asset_sync/fixture_insert_2.sql"), [TEST_CONTENT_HASH]).unwrap();
    }
    manager.request_download(test_content_hash()).unwrap();

    let conn = manager.database.get().unwrap();
    assert_eq!(client_library::assets::get_asset_requests(&conn, BlobKind::Book).unwrap(), vec![(test_content_hash(), TransferOrigin::UserInitiated)]);
}

#[test]
fn durable_download_intent_rederives_work_after_queue_recreation() {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temp.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let manager = test_manager(temp.path());
    {
        let conn = manager.database.get().unwrap();
        conn.execute(include_str!("../../sql/asset_sync/fixture_insert.sql"), [TEST_CONTENT_HASH]).unwrap();
        conn.execute(include_str!("../../sql/asset_sync/fixture_insert_2.sql"), [TEST_CONTENT_HASH]).unwrap();
    }
    manager.request_download(test_content_hash()).unwrap();
    assert!(manager.transfer_queue.statuses().is_empty(), "the queue is not the durable source of intent");
    drop(manager);

    let restored = test_manager(temp.path());
    let conn = restored.database.get().unwrap();
    assert_eq!(client_library::assets::get_asset_requests(&conn, BlobKind::Book).unwrap(), vec![(test_content_hash(), TransferOrigin::UserInitiated)]);
    assert!(LibrarySync::prepare_requested_download(&restored.assets, &conn, &test_content_hash()).unwrap().is_some());
    assert!(restored.transfer_queue.statuses().is_empty());
}
