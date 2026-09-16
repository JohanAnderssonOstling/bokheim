use super::*;
use client_platform_runtime::executor::{BackendSend, BoxedBackendFuture};
use library_files::database::{FileDatabase, FileDatabaseError, Priority};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Clone)]
struct RebindingDatabase {
    inner: DbPool,
    commands: Arc<AtomicUsize>,
}
impl FileDatabase for RebindingDatabase {
    type Connection = <DbPool as FileDatabase>::Connection;

    fn get(&self) -> Result<Self::Connection, FileDatabaseError> {
        FileDatabase::get(&self.inner)
    }

    fn command<T: BackendSend + 'static>(&self, priority: Priority, operation: impl FnOnce(&mut rusqlite::Connection) -> Result<T, FileDatabaseError> + BackendSend + 'static) -> BoxedBackendFuture<'static, Result<T, FileDatabaseError>> {
        let rebind = self.commands.fetch_add(1, Ordering::SeqCst) == 1;
        FileDatabase::command(&self.inner, priority, move |conn| {
            if rebind {
                client_library::sync_state::set_sync_metadata(conn, "bound_user_id", "new-account")?;
            }
            operation(conn)
        })
    }
}

#[tokio::test]
async fn account_change_between_snapshot_and_settlement_preserves_work() {
    let (root, database, assets, hash) = fixture();
    std::fs::write(root.path().join("book.epub"), b"local publication").unwrap();
    {
        let conn = database.get().unwrap();
        client_library::thumbnail_jobs::complete_thumbnail_work(&conn, &hash, false).unwrap();
        conn.execute_batch(include_str!("../../sql/asset_sync/snapshot_clear_file_work.sql")).unwrap();
    }
    let guarded = RebindingDatabase { inner: database.clone(), commands: Arc::new(AtomicUsize::new(0)) };
    let result = AssetPlan::next_page(&guarded, &assets, &mut PlanCursor::default()).await;
    assert!(result.err().unwrap().to_string().contains("account changed"));
    assert_eq!(guarded.commands.load(Ordering::SeqCst), 2);
    assert_eq!(client_library::assets::asset_work_page(&database.get().unwrap(), 0, i64::MAX).unwrap().len(), 1);
}
