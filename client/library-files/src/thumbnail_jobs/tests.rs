use super::*;
use crate::{test_database::TestDatabase, ROOT_DIR_ID};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::Notify;

fn fixture() -> (tempfile::TempDir, TestDatabase, AssetStore, ContentHash) {
    let root = tempfile::tempdir().unwrap();
    let assets = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    let db = TestDatabase::open(root.path().join(crate::APP_HIDDEN_LIBRARY_DB_PATH), client_library::schema::configure_connection).unwrap();
    let bytes = b"publication processed by the injected test worker";
    let hash = ContentHash::new(blake3::hash(bytes).to_hex().as_str());
    std::fs::write(root.path().join("book.epub"), bytes).unwrap();
    {
        let mut conn = db.get().unwrap();
        client_library::schema::init_db(&conn).unwrap();
        let tx = conn.transaction().unwrap();
        client_library::scanner::insert_book(&tx, hash, library_model::BookFormat::Epub).unwrap();
        client_library::scanner::insert_local_book_dir(&tx, &ROOT_DIR_ID, hash, &"book.epub".into(), 1, 1).unwrap();
        tx.execute(include_str!("sql/test_projection.sql"), rusqlite::params![hash.as_str(), ROOT_DIR_ID.to_string(), "/book.epub"]).unwrap();
        client_library::thumbnail_jobs::request_thumbnail(&tx, &hash).unwrap();
        tx.commit().unwrap();
    }
    let assets = assets.with_database_path(db.path().to_path_buf());
    (root, db, assets, hash)
}

#[derive(Default)]
struct Observer {
    active: Arc<AtomicUsize>,
    total: u64,
    completed: usize,
    ready: Vec<ContentHash>,
}
struct Activity(Arc<AtomicUsize>);
impl Drop for Activity {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
impl ThumbnailObserver for Observer {
    type Activity = Activity;
    fn prepare(&mut self, pending: u64) {
        self.total = pending;
    }
    fn begin(&mut self) -> Activity {
        self.active.fetch_add(1, Ordering::SeqCst);
        Activity(self.active.clone())
    }
    fn completed(&mut self) {
        assert_eq!(self.active.load(Ordering::SeqCst), 1);
        self.completed += 1;
    }
    fn ready(&mut self, hash: ContentHash) {
        self.ready.push(hash);
    }
}

struct StalledProcessor(Notify);
impl ThumbnailProcessor for StalledProcessor {
    fn decode(&self, _: Vec<u8>) -> BoxedBackendFuture<'_, Result<ThumbnailVersions, String>> {
        unreachable!()
    }
    fn generate(&self, _: String, _: BoxedBookReader) -> BoxedBackendFuture<'_, Result<Option<ThumbnailVersions>, String>> {
        Box::pin(async {
            self.0.notify_one();
            std::future::pending().await
        })
    }
}

#[tokio::test]
async fn cancellation_drops_batch_activity_without_consuming_the_job() {
    let (_root, db, assets, hash) = fixture();
    let processor = StalledProcessor(Notify::new());
    let mut observer = Observer::default();
    let active = observer.active.clone();
    let jobs = ThumbnailJobs::new(&db, &assets, &processor);
    let mut run = Box::pin(jobs.run_batch(&mut observer));
    tokio::select! {
        result = &mut run => panic!("worker unexpectedly finished: {result:?}"),
        _ = processor.0.notified() => {},
        _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => panic!("worker never started"),
    }
    assert_eq!(active.load(Ordering::SeqCst), 1);
    // CPU work must release placement leases before it can stall.
    let lease = tokio::time::timeout(std::time::Duration::from_secs(1), assets.lease_book(&hash)).await.unwrap().unwrap();
    drop(lease);
    drop(run);
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert_eq!(observer.completed, 0);
    assert!(observer.ready.is_empty());
    assert_eq!(client_library::thumbnail_jobs::thumbnail_work_state(&db.get().unwrap(), &hash).unwrap(), Some(("pending".into(), 0)));
}

struct FailingProcessor(AtomicUsize);
impl ThumbnailProcessor for FailingProcessor {
    fn decode(&self, _: Vec<u8>) -> BoxedBackendFuture<'_, Result<ThumbnailVersions, String>> {
        unreachable!()
    }
    fn generate(&self, _: String, _: BoxedBookReader) -> BoxedBackendFuture<'_, Result<Option<ThumbnailVersions>, String>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Err("worker failed".into()) })
    }
}

#[tokio::test]
async fn processor_failure_defers_retry_without_reporting_completion() {
    let (_root, db, assets, hash) = fixture();
    let processor = FailingProcessor(AtomicUsize::new(0));
    let jobs = ThumbnailJobs::new(&db, &assets, &processor);
    let mut observer = Observer::default();
    assert!(jobs.run_batch(&mut observer).await.is_err());
    assert_eq!(observer.total, 1);
    assert_eq!(observer.active.load(Ordering::SeqCst), 0);
    assert_eq!(observer.completed, 0);
    assert!(observer.ready.is_empty());
    let state = client_library::thumbnail_jobs::thumbnail_work_state(&db.get().unwrap(), &hash).unwrap().unwrap();
    assert_eq!(state.0, "pending");
    assert!(state.1 > 0);
    jobs.run_batch(&mut observer).await.unwrap();
    assert_eq!(processor.0.load(Ordering::SeqCst), 1, "a new pass must respect the persisted retry deadline");
}
