use super::convergence_tests::{change, db, hash, present, pull};
use library_replica::{BookLifecycleState, LibraryStateEntry};
use sync_common::{ContentHash, PullStateResponse, ROOT_DIR_ID, SyncCursor};

#[test]
fn remote_placement_restore_handles_reused_local_filename() {
    for reused in ["Shared.epub", "SHARED.EPUB"] {
        let replica = db();
        let other = ContentHash::new(&"b".repeat(64));
        let placement = |hash, present, time, seq| change(LibraryStateEntry::Placement { dir_id: ROOT_DIR_ID, content_hash: hash, present, origin_folder_id: None }, time, seq);
        pull(&replica, &[present(100, 1), change(LibraryStateEntry::BookLifecycle { content_hash: other.clone(), value: BookLifecycleState::Present }, 100, 2), placement(hash(), true, 100, 3), placement(other.clone(), true, 100, 4)]);
        replica.connection.execute("UPDATE book_dir SET file_name='Shared.epub' WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)", [hash().as_str()]).unwrap();
        pull(&replica, &[placement(hash(), false, 200, 5)]);
        replica.connection.execute("UPDATE book_dir SET file_name=?2 WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1)", rusqlite::params![other.as_str(), reused]).unwrap();
        let page = PullStateResponse { mutations: vec![placement(hash(), true, 300, 6)], next_cursor: SyncCursor::default(), has_more: false };
        let result = replica.sync_commit_pull_response(&[], &page);
        assert!(result.is_ok(), "local filenames must not block a remote placement winner: {result:?}");
        let name: String = replica.connection.query_row("SELECT file_name FROM book_dir WHERE book_row_id=(SELECT row_id FROM book WHERE content_hash=?1) AND deleted_at IS NULL", [hash().as_str()], |row| row.get(0)).unwrap();
        assert_eq!(name, hash().as_str());
        replica.sync_commit_pull_response(&[], &page).unwrap();
        assert!(replica.sync_publishable_mutations().unwrap().is_empty(), "filename repairs must not become synchronized edits");
    }
}

#[test]
fn purging_trashed_directory_purges_its_descendant_intents() {
    for empty_all in [false, true] {
        let replica = db();
        let parent = uuid::Uuid::from_u128(1);
        let child = uuid::Uuid::from_u128(2);
        replica.create_directory_with_id(&parent, &ROOT_DIR_ID, &"Parent".into()).unwrap();
        replica.create_directory_with_id(&child, &parent, &"Child".into()).unwrap();
        replica.trash_directory(&parent).unwrap();
        if empty_all {
            replica.empty_trash().unwrap();
        } else {
            replica.purge_directory(&parent).unwrap();
        }
        assert_directory_removed(&replica, parent);
        assert_directory_removed(&replica, child);
        assert!(replica.sync_publishable_mutations().unwrap().iter().any(|m| matches!(m.state.value, LibraryStateEntry::DirectoryLifecycle { dir_id, value: library_replica::DirectoryLifecycleState::Purged } if dir_id == child)));
        let later: i64 = replica.connection.query_row("SELECT MAX(changed_at)+1000 FROM sync_state_version", [], |row| row.get(0)).unwrap();
        pull(&replica, &[change(LibraryStateEntry::DirectoryParent { dir_id: child, parent_id: ROOT_DIR_ID }, later as u64, 100)]);
        let path = replica.directory_relative_path_string(&child).unwrap();
        assert_directory_removed(&replica, child);
        assert_eq!(path, None, "a parent-only update cannot undo the child's purge");
    }
}

#[test]
fn subtree_purge_terminates_for_retained_parent_cycles() {
    for empty_all in [false, true] {
        let replica = db();
        let first = uuid::Uuid::from_u128(1);
        let second = uuid::Uuid::from_u128(2);
        let unrelated = uuid::Uuid::from_u128(3);
        replica.create_directory_with_id(&unrelated, &ROOT_DIR_ID, &"Unrelated".into()).unwrap();
        replica.create_directory_with_id(&second, &ROOT_DIR_ID, &"Second".into()).unwrap();
        replica.create_directory_with_id(&first, &second, &"First".into()).unwrap();
        let later: i64 = replica.connection.query_row("SELECT MAX(changed_at)+1000 FROM sync_state_version", [], |row| row.get(0)).unwrap();
        pull(&replica, &[change(LibraryStateEntry::DirectoryParent { dir_id: second, parent_id: first }, later as u64, 100)]);
        replica.trash_directory(&first).unwrap();
        if empty_all {
            replica.empty_trash().unwrap();
        } else {
            replica.purge_directory(&first).unwrap();
        }
        assert_eq!(replica.directory_relative_path_string(&second).unwrap(), None);
        assert_eq!(replica.directory_relative_path_string(&unrelated).unwrap(), Some("Unrelated".into()));
        assert_directory_removed(&replica, first);
        assert_directory_removed(&replica, second);
    }
}

#[test]
fn subtree_purge_rolls_back_all_lifecycles_when_a_child_write_fails() {
    let replica = db();
    let parent = uuid::Uuid::from_u128(1);
    let child = uuid::Uuid::from_u128(2);
    replica.create_directory_with_id(&parent, &ROOT_DIR_ID, &"Parent".into()).unwrap();
    replica.create_directory_with_id(&child, &parent, &"Child".into()).unwrap();
    replica.trash_directory(&parent).unwrap();
    let before = replica.sync_publishable_mutations().unwrap();
    replica
        .connection
        .execute_batch(
            "CREATE TEMP TRIGGER reject_child_purge BEFORE UPDATE ON sync_state_version WHEN NEW.state_kind='directory_lifecycle' AND NEW.state_key='00000000-0000-0000-0000-000000000002' AND NEW.conflict_rank=2 BEGIN SELECT RAISE(ABORT,'injected child purge failure'); END;",
        )
        .unwrap();
    assert!(replica.purge_directory(&parent).is_err());
    let directories: i64 = replica.connection.query_row("SELECT count(*) FROM dir", [], |row| row.get(0)).unwrap();
    assert_eq!(directories, 3);
    assert_eq!(replica.sync_publishable_mutations().unwrap(), before);
    replica.connection.execute_batch("DROP TRIGGER reject_child_purge;").unwrap();
    replica.purge_directory(&parent).unwrap();
    assert_directory_removed(&replica, parent);
    assert_directory_removed(&replica, child);
}

fn assert_directory_removed(replica: &crate::Database, id: sync_common::DirId) {
    for table in ["dir", "book_dir"] {
        let column = if table == "dir" { "id" } else { "dir_id" };
        assert_eq!(replica.connection.query_row(&format!("SELECT count(*) FROM {table} WHERE {column}=?1"), [id.to_string()], |row| row.get::<_, i64>(0)).unwrap(), 0, "{table} retains purged directory {id}");
    }
    assert!(!replica.connection.prepare("PRAGMA foreign_key_check").unwrap().exists([]).unwrap());
}

#[test]
fn directory_purge_deletes_subtree_placements_and_preserves_other_memberships() {
    for empty_all in [false, true] {
        let replica = db();
        let parent = uuid::Uuid::from_u128(1);
        let child = uuid::Uuid::from_u128(2);
        replica.create_directory_with_id(&parent, &ROOT_DIR_ID, &"Parent".into()).unwrap();
        replica.create_directory_with_id(&child, &parent, &"Child".into()).unwrap();
        // One book has placements both inside and outside the purged subtree.
        pull(
            &replica,
            &[
                present(100, 1),
                change(LibraryStateEntry::Placement { dir_id: child, content_hash: hash(), present: true, origin_folder_id: None }, 100, 2),
                change(LibraryStateEntry::Placement { dir_id: ROOT_DIR_ID, content_hash: hash(), present: true, origin_folder_id: None }, 100, 3),
            ],
        );
        replica.connection.execute("INSERT INTO local_file_projection VALUES(?1,?2,'/Parent/Child/book.epub')", rusqlite::params![hash().as_str(), child.to_string()]).unwrap();
        replica.trash_directory(&parent).unwrap();
        if empty_all {
            replica.empty_trash().unwrap();
        } else {
            replica.purge_directory(&parent).unwrap();
        }
        assert_directory_removed(&replica, parent);
        assert_directory_removed(&replica, child);
        assert_eq!(replica.connection.query_row("SELECT count(*) FROM book_dir WHERE dir_id=?1 AND deleted_at IS NULL", [ROOT_DIR_ID.to_string()], |row| row.get::<_, i64>(0)).unwrap(), 1);
        assert!(replica.connection.query_row("SELECT EXISTS(SELECT 1 FROM local_file_work WHERE relative_path='/Parent/Child/book.epub' AND operation='remove')", [], |row| row.get::<_, bool>(0)).unwrap());
        let remote = db();
        let changes = replica.sync_publishable_mutations().unwrap().into_iter().map(|m| super::convergence_tests::change(m.state.value, m.state.version.changed_at, m.publisher_seq.get())).collect::<Vec<_>>();
        pull(&remote, &changes);
        assert_directory_removed(&remote, parent);
        assert_directory_removed(&remote, child);
        // New placement and parent updates cannot revive a purged directory.
        let later: i64 = replica.connection.query_row("SELECT MAX(changed_at)+1000 FROM sync_state_version", [], |row| row.get(0)).unwrap();
        pull(
            &replica,
            &[
                change(LibraryStateEntry::Placement { dir_id: child, content_hash: hash(), present: true, origin_folder_id: None }, later as u64, 100),
                change(LibraryStateEntry::DirectoryParent { dir_id: child, parent_id: ROOT_DIR_ID }, later as u64, 101),
            ],
        );
        assert_directory_removed(&replica, child);
    }
}

#[test]
fn remote_parent_purge_removes_descendants_before_their_lifecycles_arrive() {
    let replica = db();
    let parent = uuid::Uuid::from_u128(1);
    let child = uuid::Uuid::from_u128(2);
    replica.create_directory_with_id(&parent, &ROOT_DIR_ID, &"Parent".into()).unwrap();
    replica.create_directory_with_id(&child, &parent, &"Child".into()).unwrap();
    pull(&replica, &[present(100, 1), change(LibraryStateEntry::Placement { dir_id: child, content_hash: hash(), present: true, origin_folder_id: None }, 100, 2)]);
    let later: i64 = replica.connection.query_row("SELECT MAX(changed_at)+1000 FROM sync_state_version", [], |row| row.get(0)).unwrap();
    pull(&replica, &[change(LibraryStateEntry::DirectoryLifecycle { dir_id: parent, value: library_replica::DirectoryLifecycleState::Purged }, later as u64, 100)]);
    assert_directory_removed(&replica, parent);
    assert_directory_removed(&replica, child);
    // A newer explicit existence winner can recreate the materialized tree
    // and memberships from the retained canonical registers.
    pull(&replica, &[change(LibraryStateEntry::DirectoryLifecycle { dir_id: parent, value: library_replica::DirectoryLifecycleState::Present }, later as u64 + 1, 101)]);
    assert_eq!(replica.directory_relative_path_string(&child).unwrap(), Some("Parent/Child".into()));
    assert_eq!(replica.connection.query_row("SELECT count(*) FROM book_dir WHERE dir_id=?1", [child.to_string()], |row| row.get::<_, i64>(0)).unwrap(), 1);
}
