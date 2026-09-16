use super::*;
use sync_engine::SyncStore;

fn page(count: usize, revision: u64) -> PullStateResponse {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64 + 10000 + revision;
    PullStateResponse {
        mutations: (1..=count)
            .map(|id| {
                let change = library_replica::StateMutation {
                    mutation_id: MutationId::new(),
                    body: library_replica::MutationBody::Metadata { content_hash: sync_common::ContentHash::new(&format!("{id:064x}")), value: library_model::SyncBookMetadata { title: format!("Remote {revision}"), ..Default::default() } },
                    changed_at: now,
                    replica_seq: sync_common::ReplicaSeq::new(revision + id as u64).unwrap(),
                };
                sync_common::ServerMutation { mutation: change.to_wire().unwrap(), replica_id: test_support::fixture_replica_id("combined"), revision: sync_common::LibraryRevision::new(revision).unwrap() }
            })
            .collect(),
        next_cursor: SyncCursor { state_revision: Some(sync_common::LibraryRevision::new(revision).unwrap()), reading_revision: None },
        has_more: false,
    }
}

#[tokio::test]
async fn combined_response_failure_retains_ack_but_rolls_back_pull_and_cursor() {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open_library(root.path().join("library.db").to_str().unwrap(), client_library::schema::configure_connection).unwrap();
    let ids;
    {
        let conn = database.get().unwrap();
        client_library::schema::init_db(&conn).unwrap();
        conn.execute(include_str!("../sql/state_sync/combined_response_failure_retains_ack_but_rolls_back_pull_and_cursor_insert.sql"), [format!("{:064x}", 1)]).unwrap();
        ids = client_library::sync_state::get_pending_mutations(&conn).unwrap().iter().map(|c| c.mutation_id).collect::<Vec<_>>();
        conn.execute_batch(include_str!("../sql/state_sync/combined_response_failure_retains_ack_but_rolls_back_pull_and_cursor_create.sql")).unwrap();
    }
    let store = AccountStore { database: database.clone(), user: None };
    assert!(store.apply_exchange_response(&ids, &page(1, 1)).await.is_err());
    let conn = database.get().unwrap();
    assert!(client_library::sync_state::get_pending_mutations(&conn).unwrap().is_empty());
    assert_eq!(conn.query_row(include_str!("../sql/state_sync/combined_response_failure_retains_ack_but_rolls_back_pull_and_cursor_select.sql"), [], |r| r.get::<_, String>(0)).unwrap(), "Local");
    assert_eq!(client_library::sync_state::get_pull_cursor(&conn).unwrap(), SyncCursor::default());
    assert_ne!(client_library::sync_state::get_sync_metadata(&conn, "change_origin").unwrap().as_deref(), Some("remote"));
}

#[tokio::test]
async fn combined_response_does_not_ack_newer_edits_or_another_account() {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open_library(root.path().join("library.db").to_str().unwrap(), client_library::schema::configure_connection).unwrap();
    let old;
    let newer;
    {
        let conn = database.get().unwrap();
        client_library::schema::init_db(&conn).unwrap();
        conn.execute(include_str!("../sql/state_sync/benchmark_combined_response_commits_insert.sql"), [format!("{:064x}", 1)]).unwrap();
        conn.execute(include_str!("../sql/library_sync/exchange_delete.sql"), []).unwrap();
        conn.execute(include_str!("../sql/state_sync/combined_response_does_not_ack_newer_edits_or_another_account_update.sql"), []).unwrap();
        old = client_library::sync_state::get_pending_mutations(&conn).unwrap()[0].mutation_id;
        conn.execute(include_str!("../sql/state_sync/combined_response_does_not_ack_newer_edits_or_another_account_update_2.sql"), []).unwrap();
        newer = client_library::sync_state::get_pending_mutations(&conn).unwrap()[0].mutation_id;
    }
    let store = AccountStore { database: database.clone(), user: None };
    store.apply_exchange_response(&[old], &page(0, 1)).await.unwrap();
    assert_eq!(client_library::sync_state::get_pending_mutations(&database.get().unwrap()).unwrap()[0].mutation_id, newer);
    client_library::sync_state::set_sync_metadata(&database.get().unwrap(), "bound_user_id", "another-account").unwrap();
    assert!(store.apply_exchange_response(&[newer], &page(1, 2)).await.is_err());
    assert_eq!(client_library::sync_state::get_pending_mutations(&database.get().unwrap()).unwrap()[0].mutation_id, newer);
    assert_eq!(client_library::sync_state::get_pull_cursor(&database.get().unwrap()).unwrap(), page(0, 1).next_cursor);
}

#[tokio::test]
async fn native_recovery_resumes_committed_pages() {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open_library(root.path().join("library.db").to_str().unwrap(), client_library::schema::configure_connection).unwrap();
    {
        let mut conn = database.get().unwrap();
        client_library::schema::init_db(&conn).unwrap();
        let tx = client_library::schema::library_write_transaction(&mut conn).unwrap();
        for id in 1..=300 {
            tx.execute(include_str!("../sql/state_sync/native_recovery_resumes_committed_pages_insert.sql"), [format!("{id:064x}")]).unwrap();
        }
        tx.execute(include_str!("../sql/library_sync/exchange_delete.sql"), []).unwrap();
        tx.commit().unwrap();
        client_library::sync_state::begin_cursor_recovery(&conn).unwrap();
        assert_eq!(client_library::sync_state::recover_state_page(&mut conn, None).unwrap().1, 256);
    }
    let store = AccountStore { database: database.clone(), user: None };
    assert_eq!(store.complete_cursor_recovery().await.unwrap(), 44);
    let conn = database.get().unwrap();
    assert!(!client_library::sync_state::cursor_recovery_pending(&conn).unwrap());
    assert_eq!(client_library::sync_state::get_pending_mutations(&conn).unwrap().len(), 300);
}

#[tokio::test]
async fn old_repair_cannot_restore_a_checkpoint_after_rebinding() {
    let root = tempfile::tempdir().unwrap();
    let database = Database::open_library(root.path().join("library.db").to_str().unwrap(), client_library::schema::configure_connection).unwrap();
    client_library::schema::init_db(&database.get().unwrap()).unwrap();
    client_library::sync_state::set_sync_metadata(&database.get().unwrap(), "bound_user_id", "old").unwrap();
    database.get().unwrap().execute(include_str!("../sql/state_sync/old_repair_cannot_restore_a_checkpoint_after_rebinding_insert.sql"), ["a".repeat(64)]).unwrap();
    let store = AccountStore { database: database.clone(), user: Some("old".into()) };
    let cell = store.inventory_page(None).await.unwrap().into_iter().next().unwrap();
    store.save_inventory_checkpoint(Some(cell.clone())).await.unwrap();
    client_library::sync_state::rebind_sync_account(&database.get().unwrap(), "new").unwrap();
    assert!(store.save_inventory_checkpoint(Some(cell.clone())).await.is_err());
    assert!(store.enqueue_missing_state_cells(&[cell]).await.is_err());
    assert!(store.begin_cursor_recovery().await.is_err());
    assert!(store.complete_cursor_recovery().await.is_err());
    assert!(client_library::sync_state::get_sync_metadata(&database.get().unwrap(), "inventory_repair_after").unwrap().is_none());
    assert!(!client_library::sync_state::cursor_recovery_pending(&database.get().unwrap()).unwrap());
}
