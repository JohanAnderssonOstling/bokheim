use super::*;
use crate::ROOT_DIR_ID;

#[test]
fn only_new_path_changes_request_file_reconciliation() {
    let root = tempfile::tempdir().unwrap();
    let manager = test_manager(root.path());
    let database = &manager.database;
    let position = pulled_change(1, reading_change(1, 1, "epubcfi(/6/2)", 10).state.value);
    let cursor = database.sync_pull_cursor().unwrap();
    database.sync_commit_pull_response(&[], &sync_common::PullStateResponse { mutations: vec![position], next_cursor: cursor, has_more: false }).unwrap();
    for body in
        [LibraryStateEntry::DirectoryParent { dir_id: fixture_dir_id("moved"), parent_id: ROOT_DIR_ID }, LibraryStateEntry::DirectoryLifecycle { dir_id: fixture_dir_id("moved"), value: library_replica::DirectoryLifecycleState::Present }]
    {
        let cursor = database.sync_pull_cursor().unwrap();
        database.sync_commit_pull_response(&[], &sync_common::PullStateResponse { mutations: vec![pulled_change(2, body)], next_cursor: cursor, has_more: false }).unwrap();
    }
    let moved = pulled_change(2, LibraryStateEntry::DirectoryName { dir_id: fixture_dir_id("moved"), name: "Shelf".into() });
    let cursor = database.sync_pull_cursor().unwrap();
    database.sync_commit_pull_response(&[], &sync_common::PullStateResponse { mutations: vec![moved.clone()], next_cursor: cursor, has_more: false }).unwrap();
    assert_eq!(database.native_directory_work_ids().unwrap(), vec![fixture_dir_id("moved")]);
    database.clear_local_directory_work();
    let cursor = database.sync_pull_cursor().unwrap();
    database.sync_commit_pull_response(&[], &sync_common::PullStateResponse { mutations: vec![moved], next_cursor: cursor, has_more: false }).unwrap();
    assert!(!database.has_pending_file_work().unwrap(), "an already applied change must not request another walk");
    let hash = fixture_content_hash(97);
    let placement = pulled_change(3, LibraryStateEntry::Placement { dir_id: ROOT_DIR_ID, content_hash: hash, present: true, origin_folder_id: None });
    let cursor = database.sync_pull_cursor().unwrap();
    database.sync_commit_pull_response(&[], &sync_common::PullStateResponse { mutations: vec![pulled_change(1, added(97, 1)), placement], next_cursor: cursor, has_more: false }).unwrap();
    assert_eq!(database.local_book_work_hashes().unwrap(), vec![hash]);
    assert_eq!(database.native_directory_work_ids().unwrap().len(), 0, "a root placement must not reconcile the whole library");
}
