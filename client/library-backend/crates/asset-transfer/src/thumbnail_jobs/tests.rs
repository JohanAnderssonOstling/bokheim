use super::*;
use library_database::Database as TestDatabase;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use sync_common::ROOT_DIR_ID;
use tokio::sync::Notify;

const APP_HIDDEN_LIBRARY_DB_PATH: &str = ".bokheim/library.db";

fn fixture() -> (tempfile::TempDir, TestDatabase, AssetStore, ContentHash) {
    let root = tempfile::tempdir().unwrap();
    let assets = AssetStore::open(root.path().to_str().unwrap()).unwrap();
    let db = TestDatabase::open(root.path().join(APP_HIDDEN_LIBRARY_DB_PATH)).unwrap();
    let bytes = b"book processed by the injected test worker";
    let hash = ContentHash::new(blake3::hash(bytes).to_hex().as_str());
    db.initialize_library().unwrap();
    let shelf = db.create_directory(&ROOT_DIR_ID, &"Shelf".to_owned()).unwrap();
    std::fs::create_dir_all(root.path().join("Shelf")).unwrap();
    std::fs::write(root.path().join("Shelf/book.epub"), bytes).unwrap();
    db.commit_import(library_database::ImportCommit {
        parent_id: shelf.id,
        file_name: "book.epub".to_owned(),
        format: book_model::BookFormat::Epub,
        content_hash: hash,
        checksum: hash,
        size_bytes: bytes.len() as u64,
        inspection_version: 1,
        inspection: Some(book_metadata::InspectedBook {
            pdf: None,
            metadata: book_model::BookRecord { title: "Fixture".to_owned(), subtitle: None, contributors: Vec::new(), description: String::new(), book: book_model::BookMetadata::default() },
            audiobook: None,
            toc: Vec::new(),
        }),
        published: library_database::PublishedImport { name: "book.epub".to_owned(), relative_path: "Shelf/book.epub".to_owned(), published: true, fingerprint: None },
        request_thumbnail: true,
        restore_paths: Vec::new(),
    })
    .unwrap();
    let assets = assets.with_database_path(root.path().join(APP_HIDDEN_LIBRARY_DB_PATH));
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
    fn decode(&self, _: Vec<u8>) -> BoxedBackendFuture<'_, Result<ThumbnailVersions, BackendError>> {
        unreachable!()
    }
    fn generate(&self, _: String, _: BoxedBookReader) -> BoxedBackendFuture<'_, Result<Option<ThumbnailVersions>, BackendError>> {
        Box::pin(async {
            self.0.notify_one();
            std::future::pending().await
        })
    }
}

#[tokio::test]
async fn cancellation_drops_batch_activity_without_consuming_the_job() {
    cancellation_releases_activity(false).await;
}

#[tokio::test]
async fn cancellation_drops_explicit_batch_activity_without_consuming_the_job() {
    cancellation_releases_activity(true).await;
}

async fn cancellation_releases_activity(explicit: bool) {
    let (_root, db, assets, hash) = fixture();
    let processor = StalledProcessor(Notify::new());
    let mut observer = Observer::default();
    let active = observer.active.clone();
    let jobs = ThumbnailJobs::new(&db, &assets, &processor);
    let mut run = Box::pin(async {
        if explicit {
            jobs.run_members(vec![hash], &mut observer).await
        } else {
            jobs.run_batch(&mut observer).await
        }
    });
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
    assert_eq!(db.thumbnail_work_state(&hash).unwrap(), Some(("pending".into(), 0)));
}

struct FailingProcessor(AtomicUsize);
impl ThumbnailProcessor for FailingProcessor {
    fn decode(&self, _: Vec<u8>) -> BoxedBackendFuture<'_, Result<ThumbnailVersions, BackendError>> {
        unreachable!()
    }
    fn generate(&self, _: String, _: BoxedBookReader) -> BoxedBackendFuture<'_, Result<Option<ThumbnailVersions>, BackendError>> {
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
    let state = db.thumbnail_work_state(&hash).unwrap().unwrap();
    assert_eq!(state.0, "pending");
    assert!(state.1 > 0);
    jobs.run_batch(&mut observer).await.unwrap();
    assert_eq!(processor.0.load(Ordering::SeqCst), 1, "a new pass must respect the persisted retry deadline");
    assert!(jobs.run_members(vec![hash], &mut observer).await.is_err());
    assert_eq!(processor.0.load(Ordering::SeqCst), 2, "explicit members retain their independent selection rules");
    assert_eq!(observer.active.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn empty_explicit_batch_does_not_reset_progress() {
    let (_root, db, assets, _) = fixture();
    let processor = FailingProcessor(AtomicUsize::new(0));
    let jobs = ThumbnailJobs::new(&db, &assets, &processor);
    let mut observer = Observer { total: 42, ..Default::default() };
    jobs.run_members(Vec::new(), &mut observer).await.unwrap();
    assert_eq!(observer.total, 42);
    assert_eq!(processor.0.load(Ordering::SeqCst), 0);
    assert_eq!(observer.active.load(Ordering::SeqCst), 0);
}
