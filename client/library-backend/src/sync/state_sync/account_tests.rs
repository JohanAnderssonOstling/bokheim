use std::path::Path;
use super::*;
use crate::sync::library_sync::LibrarySyncConfig;
use crate::sync::LibrarySync;
use std::sync::{Arc, RwLock};
use sync_engine::SyncStore;

fn page(count: usize, revision: u64) -> PullStateResponse {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64 + 10000 + revision;
    PullStateResponse {
        mutations: (1..=count)
            .map(|id| addition(sync_common::ContentHash::new(&format!("{id:064x}")), 0))
            .chain((1..=count).map(|id| {
                let change =
                    (library_replica::LibraryStateEntry::Metadata { content_hash: sync_common::ContentHash::new(&format!("{id:064x}")), value: library_replica::BookDetails { title: format!("Remote {revision}"), ..Default::default() } })
                        .publication(sync_common::ReplicaId::nil(), now, sync_common::ReplicaSeq::new(revision + id as u64).unwrap());
                sync_common::fixture_delivery(
                    sync_common::transport::representation::<_, sync_common::OpaquePublication>(&(change)).unwrap(),
                    test_support::fixture_replica_id("combined"),
                    sync_common::LibraryRevision::new(revision).unwrap(),
                )
            }))
            .collect(),
        next_cursor: SyncCursor { lifecycle_revision: Some(sync_common::LibraryRevision::new(revision).unwrap()), field_revision: Some(sync_common::LibraryRevision::new(revision).unwrap()) },
        has_more: false,
    }
}

fn addition(hash: sync_common::ContentHash, changed_at: u64) -> sync_common::StateDelivery {
    let change = (library_replica::LibraryStateEntry::BookLifecycle { content_hash: hash, value: library_replica::BookLifecycleState::Present }).publication(
        sync_common::ReplicaId::nil(),
        changed_at,
        sync_common::ReplicaSeq::new(changed_at.max(1)).unwrap(),
    );
    sync_common::fixture_delivery(sync_common::transport::representation::<_, sync_common::OpaquePublication>(&(change)).unwrap(), test_support::fixture_replica_id("combined"), sync_common::LibraryRevision::new(1).unwrap())
}

/// Raw second connection for fixtures and assertions only. Production paths
/// below only touch the owner.
fn raw(db_path: &Path) -> library_database::rusqlite::Connection {
    // Configure the query functions on the independent fixture connection.
    library_database::open_fixture_connection(db_path).unwrap()
}

/// Test handle pairing the session-owned database with its sync worker.
/// Method calls dereference to `LibrarySync`; storage travels explicitly.
struct TestSync {
    sync: LibrarySync,
    database: library_database::Database,
}

impl std::ops::Deref for TestSync {
    type Target = LibrarySync;
    fn deref(&self) -> &LibrarySync {
        &self.sync
    }
}

fn test_sync(root: &Path) -> TestSync {
    let db_path = root.join("library.db");
    let database = library_database::Database::open(&db_path).unwrap();
    database.initialize_library().unwrap();
    let assets = crate::asset_store::open(root.to_str().unwrap()).unwrap();
    let sync = LibrarySync::new(LibrarySyncConfig {
        database_path: db_path.clone(),
        assets,
        replica_id: test_support::fixture_replica_id("account-tests"),
        server_url: account_client::ServerUrl::parse("http://127.0.0.1:9").unwrap(),
        library_id: test_support::fixture_library_id("library"),
        event_tx: None,
        transfer_queue: Default::default(),
        credentials: Arc::new(RwLock::new(None::<sync_transport::SyncCredentials>)),
        account: None,
        notification_interest: Default::default(),
        cpu: std::sync::Arc::new(client_platform_native::cpu_host::NativeHost::default()),
    });
    TestSync { sync, database }
}

fn outbox_ids(db_path: &Path) -> Vec<u64> {
    raw(db_path).prepare("SELECT replica_seq FROM sync_outbox ORDER BY replica_seq").unwrap().query_map([], |row| row.get::<_, i64>(0).map(|seq| seq as u64)).unwrap().collect::<Result<Vec<u64>, _>>().unwrap()
}

#[tokio::test]
async fn combined_response_failure_retains_ack_but_rolls_back_pull_and_cursor() {
    let root = tempfile::tempdir().unwrap();
    let db_path = root.path().join("library.db");
    let sync = test_sync(root.path());
    let hash = sync_common::ContentHash::new(&format!("{:064x}", 1));
    sync.database
        .commit_import(library_database::ImportCommit {
            parent_id: sync_common::ROOT_DIR_ID,
            file_name: "local.epub".into(),
            format: book_model::BookFormat::Epub,
            content_hash: hash,
            checksum: hash,
            size_bytes: 1,
            inspection_version: 1,
            inspection: Some(book_metadata::InspectedBook {
                pdf: None,
                audiobook: None,
                toc: Vec::new(),
                metadata: book_model::BookRecord { metadata: book_model::BookDetails { title: "Local".into(), ..Default::default() }, description: String::new() },
            }),
            published: library_database::PublishedImport { name: "local.epub".into(), relative_path: "/local.epub".into(), published: false, fingerprint: None },
            request_thumbnail: false,
            restore_paths: Vec::new(),
        })
        .unwrap();
    // Pull cannot record its first winner. The acknowledgement still commits
    // separately, while the failed pull leaves its cursor and projection intact.
    raw(&db_path).execute_batch("DROP TABLE sync_state_version").unwrap();
    let ids: Vec<ReplicaSeq> = outbox_ids(&db_path).into_iter().map(|id| ReplicaSeq::new(id).unwrap()).collect();
    assert!(!ids.is_empty(), "canonical import must publish local state to acknowledge");
    let store = AccountStore { sync: &sync.sync };
    assert!(store.apply_exchange_response(&ids, &page(1, 1)).await.is_err());
    assert!(outbox_ids(&db_path).is_empty(), "the acknowledgement is retried under a fresh transaction");
    let conn = raw(&db_path);
    assert_eq!(conn.query_row("SELECT title FROM book WHERE content_hash = ?1", [hash.as_str()], |row| row.get::<_, String>(0)).unwrap(), "Local");
    assert_eq!(sync.database.sync_pull_cursor().unwrap(), SyncCursor::default());
}

#[tokio::test]
async fn combined_response_applies_regardless_of_account() {
    let root = tempfile::tempdir().unwrap();
    let db_path = root.path().join("library.db");
    let sync = test_sync(root.path());
    {
        let conn = raw(&db_path);
        conn.execute("INSERT INTO sync_outbox(replica_seq, state_kind, state_key, state_subkey, body, changed_at) VALUES (2, 'metadata', ?1, '', X'00', 10)", [format!("{:064x}", 1).as_str()]).unwrap();
        conn.execute("INSERT INTO sync_outbox(replica_seq, state_kind, state_key, state_subkey, body, changed_at) VALUES (3, 'metadata', ?1, '', X'00', 20)", [format!("{:064x}", 2).as_str()]).unwrap();
    }
    let store = AccountStore { sync: &sync.sync };
    let ids: Vec<ReplicaSeq> = outbox_ids(&db_path).into_iter().map(|id| ReplicaSeq::new(id).unwrap()).collect();
    let old = ids[0];
    let newer = ids[1];
    store.apply_exchange_response(&[old], &page(0, 1)).await.unwrap();
    assert_eq!(outbox_ids(&db_path), vec![newer.get()]);
    // No account is bound: applying a response created for another account
    // still advances the shared cursor and acknowledges the mutations.
    store.apply_exchange_response(&[newer], &page(1, 2)).await.unwrap();
    assert!(outbox_ids(&db_path).is_empty());
    assert_eq!(sync.database.sync_pull_cursor().unwrap(), page(1, 2).next_cursor);
}

#[tokio::test]
async fn native_recovery_resumes_committed_pages() {
    let root = tempfile::tempdir().unwrap();
    let sync = test_sync(root.path());
    // Remote additions write versions without outbox work, so recovery has
    // exactly the committed pages to re-enqueue.
    let incoming: Vec<sync_common::StateDelivery> = (1..=300).map(|id| addition(sync_common::ContentHash::new(&format!("{id:064x}")), 1000 + id as u64)).collect();
    let next = SyncCursor { lifecycle_revision: Some(sync_common::LibraryRevision::new(9).unwrap()), field_revision: None };
    assert!(sync.database.sync_commit_pull_response(&[], &PullStateResponse { mutations: incoming, next_cursor: next, has_more: false }).unwrap());
    assert!(sync.database.sync_publishable_mutations().unwrap().is_empty());
    sync.database.sync_begin_cursor_recovery().unwrap();
    let store = AccountStore { sync: &sync.sync };
    assert_eq!(store.complete_cursor_recovery().await.unwrap(), 300);
    assert!(!sync.database.sync_cursor_recovery_pending().unwrap());
    assert_eq!(sync.database.sync_publishable_mutations().unwrap().len(), 300);
}

#[tokio::test]
async fn cursor_recovery_survives_account_switch() {
    let root = tempfile::tempdir().unwrap();
    let sync = test_sync(root.path());
    let hash = sync_common::ContentHash::new(&format!("{:064x}", 7));
    let next = SyncCursor { lifecycle_revision: Some(sync_common::LibraryRevision::new(9).unwrap()), field_revision: None };
    assert!(sync.database.sync_commit_pull_response(&[], &PullStateResponse { mutations: vec![addition(hash, 100)], next_cursor: next, has_more: false }).unwrap());
    let store = AccountStore { sync: &sync.sync };
    // Switching accounts is a non-event: the cursor and its recovery keep
    // working against the same durable state.
    store.begin_cursor_recovery().await.unwrap();
    assert!(sync.database.sync_cursor_recovery_pending().unwrap());
}
