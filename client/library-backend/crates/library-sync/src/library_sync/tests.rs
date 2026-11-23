use super::*;
use account_client::{Session, SessionPersistence};
use client_platform_runtime::executor::BackendExecutor;
use library_database::DatabaseClient;
use library_services::{
    account::{LibraryAccount, LibraryAccountSession},
    events::LibraryEventState,
};
use std::io;
use sync_common::{DirId, ROOT_DIR_ID, SyncCursor};

#[derive(Clone, Debug)]
struct Persistence;
impl SessionPersistence for Persistence {
    fn load_account_session(&self) -> io::Result<Option<Session>> {
        Ok(None)
    }
    fn save_account_session(&self, _: &Session) -> io::Result<()> {
        Ok(())
    }
    fn clear_account_session(&self) -> io::Result<()> {
        Ok(())
    }
}

async fn fixture(server_url: Url, account: LibraryAccount) -> (tempfile::TempDir, LibrarySync, LibraryEventState) {
    let root = tempfile::tempdir().unwrap();
    let database = DatabaseClient::open(root.path().join("library.sqlite3"), &BackendExecutor::new().unwrap()).await.unwrap();
    database.initialize_library().await.unwrap();
    database.create_directory_with_id(&DirId::from_u128(1), &ROOT_DIR_ID, &"Offline shelf".into()).await.unwrap();
    database.set_last_successful_sync_at(123).await.unwrap();
    let (events, state) = LibraryEventRouter::new();
    let sync = LibrarySync::new(LibrarySyncConfig { database, events, http_client: http_response::client(), server_url, library_id: LibraryId::new_v4(), account, notification_interest: Default::default() }).await.unwrap();
    (root, sync, state)
}

#[tokio::test]
async fn signed_out_passes_leave_queued_changes_and_sync_status_untouched() {
    let server: Url = "https://example.com".parse().unwrap();
    let session = LibraryAccountSession::load(Persistence, server.clone()).unwrap();
    let (root, sync, mut state) = fixture(server, session.library_account()).await;
    let updates = state.updates.subscribe();
    let watermark = sync.database.sync_outbox_watermark().await.unwrap();
    assert!(watermark > 0);
    let probe = rusqlite::Connection::open(root.path().join("library.sqlite3")).unwrap();
    probe.execute_batch("CREATE TRIGGER reject_signed_out_preparation BEFORE UPDATE ON sync_outbox BEGIN SELECT RAISE(ABORT, 'signed-out sync must not prepare the outbox'); END;").unwrap();
    for outbox_only in [false, true] {
        assert!(sync.synchronize_state_mode(outbox_only).await.unwrap().mutation_issues.is_empty());
        state.drain(&sync.events);
        assert!(updates.try_recv().is_err());
        assert_eq!(sync.database.sync_outbox_watermark().await.unwrap(), watermark);
        assert_eq!(sync.database.sync_pull_cursor().await.unwrap(), SyncCursor::default());
        assert_eq!(sync.database.stored_sync_status().await.unwrap().last_synced_at, Some(123));
    }
}

#[tokio::test]
async fn identity_snapshots_follow_account_switches_and_reject_another_server() {
    let server: Url = "https://example.com".parse().unwrap();
    let account = LibraryAccountSession::load(Persistence, server.clone()).unwrap();
    let session = |user: &str, token: char| Session::try_new(server.clone(), user.into(), format!("{user}@example.com"), token.to_string().repeat(43), i64::MAX - 1, "r".repeat(43), i64::MAX).unwrap();
    account.set(session("first", 'a')).unwrap();
    let capability = account.library_account();
    let (_root, mut sync, mut state) = fixture(server.clone(), capability).await;
    let updates = state.updates.subscribe();
    let watermark = sync.database.sync_outbox_watermark().await.unwrap();
    let first_credentials = sync.sync_credentials().await.unwrap().unwrap();
    account.set(session("second", 'b')).unwrap();
    let second_credentials = sync.sync_credentials().await.unwrap().unwrap();
    assert_eq!(first_credentials.access_token(), "a".repeat(43));
    assert_eq!(second_credentials.access_token(), "b".repeat(43));
    assert_eq!(second_credentials.server_url(), &server);
    sync.server_url = "https://other.example".parse().unwrap();
    for outbox_only in [false, true] {
        let error = sync.synchronize_state_mode(outbox_only).await.unwrap_err();
        assert!(error.to_string().contains("account belongs to another server"));
    }
    state.drain(&sync.events);
    assert!(updates.try_recv().is_err());
    assert_eq!(sync.database.sync_outbox_watermark().await.unwrap(), watermark);
    assert_eq!(sync.database.stored_sync_status().await.unwrap().last_synced_at, Some(123));
    account.take().unwrap();
    assert!(sync.sync_credentials().await.unwrap().is_none());
}
