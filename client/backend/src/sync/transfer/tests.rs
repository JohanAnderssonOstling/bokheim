use super::*;
use crate::{BlobKind, RelativeBookPath, ServerUrl};
use axum::body::Body;
use axum::http::{HeaderMap, Response, StatusCode};
use axum::routing::{get, post};
use axum::Router;
use std::sync::atomic::{AtomicBool, Ordering};
use test_support::fixture_content_hash;

const TEST_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn test_session(server_url: &ServerUrl) -> Arc<std::sync::RwLock<Option<sync_client::Session>>> {
    Arc::new(std::sync::RwLock::new(Some(sync_client::Session::try_new(server_url.clone(), "test-user".to_string(), "test-reader".to_string(), TEST_TOKEN.to_string()).unwrap())))
}

fn queued_upload(hash: &str, origin: TransferOrigin) -> TransferJob {
    TransferJob::upload_blob(ContentHash::new(&hash), "/tmp/book.epub".to_owned(), None, origin)
}

#[test]
fn ephemeral_queue_deduplicates_and_preserves_user_priority() {
    let queue = TransferQueue::default();
    let hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    queue.reconcile(vec![queued_upload(hash, TransferOrigin::Background), queued_upload(hash, TransferOrigin::UserInitiated)]);
    queue.reconcile(vec![queued_upload(hash, TransferOrigin::Background)]);

    let statuses = queue.statuses();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].origin, TransferOrigin::UserInitiated);
    assert_eq!(statuses[0].state, TransferState::Queued);
}

#[test]
fn transfer_statuses_are_aggregated_with_stable_batch_progress() {
    let queue = TransferQueue::default();
    let first = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let second = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    queue.reconcile(vec![queued_upload(first, TransferOrigin::Background), queued_upload(second, TransferOrigin::Background)]);
    let initial = queue.statuses();
    assert_eq!(initial.len(), 1);
    assert_eq!((initial[0].completed_items, initial[0].total_items), (0, 2));

    let completed = queue.claim_due(true).unwrap();
    queue.complete(completed.id, None);
    let progressed = queue.statuses();
    assert_eq!(progressed.len(), 1);
    assert_eq!((progressed[0].completed_items, progressed[0].total_items), (1, 2));

    let completed = queue.claim_due(true).unwrap();
    queue.complete(completed.id, None);
    assert!(queue.statuses().is_empty());
    let history = queue.history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].state, TransferState::Completed);
    assert_eq!((history[0].completed_items, history[0].total_items), (2, 2));
}

#[test]
fn completed_batch_history_survives_queue_recreation() {
    let root = tempfile::tempdir().unwrap();
    let database = crate::platform::database::Database::open(root.path().join("library.db"), db::configure_connection).unwrap();
    db::init_db(&database.get().unwrap()).unwrap();
    let queue = TransferQueue::with_history_db(database.clone());
    let hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    queue.reconcile(vec![queued_upload(hash, TransferOrigin::Background)]);
    let completed = queue.claim_due(true).unwrap();
    queue.complete(completed.id, None);
    drop(queue);

    let restored = TransferQueue::with_history_db(database);
    let history = restored.history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].state, TransferState::Completed);
    assert_eq!((history[0].completed_items, history[0].total_items), (1, 1));
}

#[test]
fn thumbnail_network_work_is_claimed_in_bounded_batches() {
    let queue = TransferQueue::default();
    let jobs = (0..20).map(|index| TransferJob::download_thumbnail(fixture_content_hash(index), fixture_content_hash(index + 100), TransferOrigin::Background)).collect();
    queue.reconcile(jobs);
    assert_eq!(queue.claim_due_batch(true).len(), MAX_THUMBNAIL_BATCH_ENTRIES);
    assert_eq!(queue.claim_due_batch(true).len(), 20 - MAX_THUMBNAIL_BATCH_ENTRIES);
}

#[test]
fn reconciliation_removes_obsolete_jobs_from_batch_progress() {
    let queue = TransferQueue::default();
    let first = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let second = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let obsolete = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    queue.reconcile(vec![queued_upload(first, TransferOrigin::Background), queued_upload(second, TransferOrigin::Background), queued_upload(obsolete, TransferOrigin::Background)]);

    let completed = queue.claim_due(true).unwrap();
    queue.complete(completed.id, None);
    queue.reconcile(vec![queued_upload(second, TransferOrigin::Background)]);

    let status = queue.statuses().pop().unwrap();
    assert_eq!((status.completed_items, status.total_items), (1, 2));
}

#[test]
fn thumbnails_are_claimed_before_user_requested_books() {
    let queue = TransferQueue::default();
    let thumbnail_hash = fixture_content_hash(1);
    let book_hash = fixture_content_hash(2);
    queue.reconcile(vec![
        TransferJob::download_blob(book_hash, BookPlacement::new(book_hash, RelativeBookPath::parse("/book.epub").unwrap()), TransferOrigin::UserInitiated),
        TransferJob::download_thumbnail(thumbnail_hash, book_hash, TransferOrigin::Background),
    ]);

    let claimed = queue.claim_due(true).unwrap();
    assert!(matches!(claimed.job.operation, TransferOperation::DownloadThumbnail { .. }));
}

#[test]
fn ephemeral_queue_pauses_background_jobs_and_tracks_retries() {
    let queue = TransferQueue::default();
    let background = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let manual = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    queue.reconcile(vec![queued_upload(background, TransferOrigin::Background), queued_upload(manual, TransferOrigin::UserInitiated)]);

    let claimed = queue.claim_due(false).expect("manual transfer remains eligible");
    assert_eq!(claimed.job.origin(), TransferOrigin::UserInitiated);
    assert!(queue.retry(claimed.id, "network unavailable".to_owned()).is_some());
    let retrying = queue.statuses().into_iter().find(|status| status.content_hash == ContentHash::new(manual)).unwrap();
    assert_eq!(retrying.state, TransferState::Retrying);
    assert_eq!(retrying.attempts, 1);
    assert_eq!(retrying.last_error.as_deref(), Some("network unavailable"));
    assert!(queue.claim_due(false).is_none());

    let background = queue.claim_due(true).expect("background transfer resumes when allowed");
    assert_eq!(background.job.origin(), TransferOrigin::Background);
}

#[test]
fn authentication_pause_does_not_consume_retry_budget() {
    let queue = TransferQueue::default();
    let hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    queue.reconcile(vec![queued_upload(hash, TransferOrigin::Background)]);
    let rejected = queue.claim_due(true).unwrap();
    queue.release(std::slice::from_ref(&rejected));
    queue.pause_authentication();

    assert!(!queue.try_start_scheduler(), "authentication pauses the queue rather than changing a job");
    queue.resume_authentication();
    assert!(queue.try_start_scheduler());

    let retried = queue.claim_due(true).unwrap();
    assert_eq!(retried.id, rejected.id);
    assert_eq!(queue.statuses()[0].attempts, 0);
}

#[test]
fn only_one_scheduler_owns_the_queue() {
    let queue = TransferQueue::default();
    assert!(queue.try_start_scheduler());
    assert!(!queue.try_start_scheduler());
}

#[test]
fn thumbnail_paths_are_derived_from_the_validated_library_root() {
    let root = tempfile::tempdir().unwrap();
    let library_root = root.path().canonicalize().unwrap();
    let content_hash = ContentHash::new(&"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(thumbnail_path_from_content_hash(&library_root, &content_hash), root.path().canonicalize().unwrap().join(format!("{}/thumbnails/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.jpg", crate::APP_HIDDEN_DIR)));
}

/// The upload path deliberately does not inspect book format; the library is
/// a catalogue of the user's files, not an upload trust boundary. What it
/// must still refuse is bytes that do not match the content hash they claim
/// to be, and it must refuse them before any network I/O.
#[tokio::test]
async fn upload_rejects_bytes_that_do_not_match_their_content_hash_before_network_io() {
    let root = tempfile::tempdir().unwrap();
    let book_path = root.path().join("book.epub");
    std::fs::write(&book_path, b"bytes that hash to something else").unwrap();
    let database = crate::platform::database::Database::open(root.path().join("library.db"), db::configure_connection).unwrap();
    db::init_db(&database.get().unwrap()).unwrap();
    let server_url = ServerUrl::parse("http://127.0.0.1:9").unwrap();
    let worker = TransferWorker::new(TransferWorkerConfig {
        http_client: Client::new(),
        library_id: test_support::fixture_library_id("library"),
        library_root: root.path().canonicalize().unwrap().to_string_lossy().into_owned(),
        db_pool: database.clone(),
        event_tx: None,
        session: test_session(&server_url),
        runtime: TransferRuntime::default(),
        queue: Arc::new(TransferQueue::default()),
    });
    let expected = ContentHash::new(&"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

    let error = worker.upload_blob(&expected, book_path.to_str().unwrap(), None).await.unwrap_err();

    assert!(error.to_string().contains("upload hash mismatch"), "unexpected error: {error}");
    // The rejection records that these bytes are not the claimed content, so
    // no reference may claim the blob is present on this device.
    let present_refs: i64 = database.get().unwrap().query_row("SELECT COUNT(*) FROM blob_local_ref WHERE kind='book' AND hash=?1 AND exists_flag=1", [expected.as_str()], |row| row.get(0)).unwrap();
    assert_eq!(present_refs, 0);
}

fn worker_with_placement(root: &Path, server_url: ServerUrl, content_hash: &ContentHash) -> (TransferWorker, DbPool) {
    let database = crate::platform::database::Database::open(root.join(crate::APP_HIDDEN_LIBRARY_DB_PATH), db::configure_connection).unwrap();
    let conn = database.get().unwrap();
    db::init_db(&conn).unwrap();
    let dir_id = test_support::fixture_dir_id("d1").to_string();
    conn.execute("INSERT INTO dir(id, parent_id, name) VALUES (?1, '00000000-0000-0000-0000-000000000000', 'Shelf')", [dir_id.as_str()]).unwrap();
    conn.execute("INSERT INTO book(content_hash, added_at, format) VALUES (?1, 1, 'epub')", [content_hash.as_str()]).unwrap();
    conn.execute("INSERT INTO book_dir(dir_id, book_row_id, file_name, local_hash, is_downloaded) VALUES (?2, (SELECT row_id FROM book WHERE content_hash=?1), 'book.epub', '', 0)", rusqlite::params![content_hash.as_str(), dir_id]).unwrap();
    drop(conn);
    let worker = TransferWorker::new(TransferWorkerConfig {
        http_client: Client::new(),
        library_id: test_support::fixture_library_id("library"),
        library_root: root.canonicalize().unwrap().to_string_lossy().into_owned(),
        db_pool: database.clone(),
        event_tx: None,
        session: test_session(&server_url),
        runtime: TransferRuntime::default(),
        queue: Arc::new(TransferQueue::default()),
    });
    (worker, database)
}

#[tokio::test]
async fn complete_download_uses_shared_sqlite_storage_and_native_projection() {
    let bytes = b"a complete epub payload stored by the common transfer worker".to_vec();
    let content_hash = ContentHash::new(&blake3::hash(&bytes).to_hex().as_str());
    let saw_request = Arc::new(AtomicBool::new(false));
    let route_bytes = Arc::new(bytes.clone());
    let route_saw_request = Arc::clone(&saw_request);
    let route = format!("/api/libraries/{}/blobs/:hash", test_support::fixture_library_id("library"));
    let app = Router::new().route(
        &route,
        get(move |headers: HeaderMap| {
            let bytes = Arc::clone(&route_bytes);
            let saw_request = Arc::clone(&route_saw_request);
            async move {
                assert!(headers.get("range").is_none());
                saw_request.store(true, Ordering::SeqCst);
                Response::builder().status(StatusCode::OK).body(Body::from(bytes.as_ref().clone())).unwrap()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let (worker, pool) = worker_with_placement(root.path(), ServerUrl::parse(&format!("http://{address}")).unwrap(), &content_hash);
    let placement = BookPlacement::new(content_hash, RelativeBookPath::parse("/Shelf/book.epub").unwrap());

    worker.download_blob(&content_hash, &placement).await.unwrap();

    assert!(saw_request.load(Ordering::SeqCst));
    assert_eq!(db::get_local_blob(&pool.get().unwrap(), BlobKind::Book, &content_hash).unwrap().unwrap(), bytes);
    assert_eq!(std::fs::read(root.path().join("Shelf/book.epub")).unwrap(), bytes);
    server.abort();
}

#[tokio::test]
async fn projection_conflict_preserves_user_file_and_commits_verified_sqlite_bytes() {
    let bytes = b"verified remote publication".to_vec();
    let content_hash = ContentHash::new(blake3::hash(&bytes).to_hex().as_str());
    let route_bytes = Arc::new(bytes.clone());
    let route = format!("/api/libraries/{}/blobs/:hash", test_support::fixture_library_id("library"));
    let app = Router::new().route(
        &route,
        get(move || {
            let bytes = Arc::clone(&route_bytes);
            async move { bytes.as_ref().clone() }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    std::fs::create_dir_all(root.path().join("Shelf")).unwrap();
    std::fs::write(root.path().join("Shelf/book.epub"), b"unrelated user file").unwrap();
    let (worker, pool) = worker_with_placement(root.path(), ServerUrl::parse(&format!("http://{address}")).unwrap(), &content_hash);
    let placement = BookPlacement::new(content_hash, RelativeBookPath::parse("/Shelf/book.epub").unwrap());
    db::add_asset_request(&pool.get().unwrap(), BlobKind::Book, &content_hash, TransferOrigin::UserInitiated).unwrap();

    worker.download_blob(&content_hash, &placement).await.unwrap();

    assert_eq!(std::fs::read(root.path().join("Shelf/book.epub")).unwrap(), b"unrelated user file");
    let conn = pool.get().unwrap();
    assert_eq!(db::get_local_blob(&conn, BlobKind::Book, &content_hash).unwrap().unwrap(), bytes);
    assert!(db::is_book_downloaded(&conn, content_hash).unwrap());
    assert!(db::get_asset_requests(&conn, BlobKind::Book).unwrap().is_empty());
    server.abort();
}

#[tokio::test]
async fn missing_remote_blob_resolves_durable_intent_instead_of_recreating_a_poison_job() {
    let content_hash = fixture_content_hash(41);
    let route = format!("/api/libraries/{}/blobs/:hash", test_support::fixture_library_id("library"));
    let app = Router::new().route(&route, get(|| async { StatusCode::NOT_FOUND }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let (worker, pool) = worker_with_placement(root.path(), ServerUrl::parse(&format!("http://{address}")).unwrap(), &content_hash);
    let placement = BookPlacement::new(content_hash, RelativeBookPath::parse("/Shelf/book.epub").unwrap());
    {
        let conn = pool.get().unwrap();
        db::add_asset_request(&conn, BlobKind::Book, &content_hash, TransferOrigin::UserInitiated).unwrap();
        db::record_remote_asset(&conn, BlobKind::Book, &content_hash).unwrap();
    }

    worker.download_blob(&content_hash, &placement).await.unwrap();

    let conn = pool.get().unwrap();
    assert!(db::get_asset_requests(&conn, BlobKind::Book).unwrap().is_empty());
    assert!(!db::get_remote_asset_hashes(&conn, BlobKind::Book).unwrap().contains(&content_hash));
    server.abort();
}

#[tokio::test]
async fn corrupt_remote_blob_invalidates_remote_fact_and_resolves_request() {
    let expected = fixture_content_hash(42);
    let route = format!("/api/libraries/{}/blobs/:hash", test_support::fixture_library_id("library"));
    let app = Router::new().route(&route, get(|| async { b"bytes with the wrong content hash" }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let (worker, pool) = worker_with_placement(root.path(), ServerUrl::parse(&format!("http://{address}")).unwrap(), &expected);
    let placement = BookPlacement::new(expected, RelativeBookPath::parse("/Shelf/book.epub").unwrap());
    {
        let conn = pool.get().unwrap();
        db::add_asset_request(&conn, BlobKind::Book, &expected, TransferOrigin::UserInitiated).unwrap();
        db::record_remote_asset(&conn, BlobKind::Book, &expected).unwrap();
    }

    let error = worker.download_blob(&expected, &placement).await.unwrap_err();

    assert!(matches!(error, TransferError::Rejected(_)));
    let conn = pool.get().unwrap();
    assert!(db::get_asset_requests(&conn, BlobKind::Book).unwrap().is_empty());
    assert!(!db::get_remote_asset_hashes(&conn, BlobKind::Book).unwrap().contains(&expected));
    server.abort();
}

#[tokio::test]
async fn unauthorized_transfer_pauses_the_scheduler_without_changing_the_job() {
    let route = format!("/api/libraries/{}/thumbnail-batch", test_support::fixture_library_id("library"));
    let app = Router::new().route(&route, post(|| async { StatusCode::UNAUTHORIZED }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let content_hash = ContentHash::new(&"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let (worker, _) = worker_with_placement(root.path(), ServerUrl::parse(&format!("http://{address}")).unwrap(), &content_hash);

    let job = TransferJob::download_thumbnail(content_hash, fixture_content_hash(7), TransferOrigin::Background);
    let error = worker.clone().run(vec![job], 1).await.unwrap_err();

    assert!(matches!(error, TransferError::AuthenticationRequired));
    assert!(worker.session.read().unwrap().is_some(), "the account layer must retain the refresh token so it can renew access before retrying");
    let status = worker.queue.statuses().pop().unwrap();
    assert_eq!(status.state, TransferState::Queued);
    assert_eq!(status.attempts, 0);
    assert!(!worker.queue.try_start_scheduler(), "the queue stays globally paused until authentication is explicitly resumed");
    server.abort();
}

#[tokio::test]
async fn completed_download_is_not_published_after_placement_becomes_stale() {
    let bytes = b"verified but stale bytes".to_vec();
    let content_hash = ContentHash::new(&blake3::hash(&bytes).to_hex().as_str());
    let route_bytes = Arc::new(bytes);
    let route = format!("/api/libraries/{}/blobs/:hash", test_support::fixture_library_id("library"));
    let app = Router::new().route(
        &route,
        get(move || {
            let bytes = Arc::clone(&route_bytes);
            async move { Response::builder().status(StatusCode::OK).body(Body::from(bytes.as_ref().clone())).unwrap() }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join(crate::APP_HIDDEN_DIR)).unwrap();
    let (worker, pool) = worker_with_placement(root.path(), ServerUrl::parse(&format!("http://{address}")).unwrap(), &content_hash);
    pool.get().unwrap().execute("UPDATE book_dir SET deleted_at=1", []).unwrap();
    let placement = BookPlacement::new(fixture_content_hash(7), RelativeBookPath::parse("/Shelf/book.epub").unwrap());

    worker.download_blob(&content_hash, &placement).await.unwrap();

    assert!(!root.path().join("Shelf/book.epub").exists());
    assert!(db::get_local_blob(&pool.get().unwrap(), BlobKind::Book, &content_hash).unwrap().is_none());
    server.abort();
}

#[test]
fn failure_classification_is_closed_and_explicit() {
    assert!(matches!(TransferError::rejected("bad input"), TransferError::Rejected(_)));
    let io_error = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "offline");
    assert!(matches!(TransferError::from(io_error), TransferError::Retryable(_)));
}
