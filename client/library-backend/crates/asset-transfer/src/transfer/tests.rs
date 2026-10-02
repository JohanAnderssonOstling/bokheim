use std::sync::Arc;
use library_runtime::transfer_history::queue_with_history;
use library_model::RelativeBookPath;
use crate::{TransferError, TransferJob, TransferOperation, TransferQueue};
use library_model::{TransferOrigin, TransferState};
use library_model::BookPlacement;
use sync_common::{api::assets::MAX_THUMBNAIL_BATCH_ENTRIES, ContentHash};
use axum::http::StatusCode;
use axum::routing::get;
use axum::Router;
use test_support::fixture_content_hash;

fn queued_upload(hash: &str, origin: TransferOrigin) -> TransferJob {
    TransferJob::upload_blob(ContentHash::new(&hash), origin)
}

/// The transfer stack speaks replica values; fixtures speak session values.
/// Conversion is field-identical and test-only.

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

    let completed = queue.claim_due().unwrap();
    queue.complete(completed.id, None);
    let progressed = queue.statuses();
    assert_eq!(progressed.len(), 1);
    assert_eq!((progressed[0].completed_items, progressed[0].total_items), (1, 2));

    let completed = queue.claim_due().unwrap();
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
    let history_path = root.path().join("transfer-history-v1.msgpack");
    let db = Arc::new(library_database::Database::open(root.path().join("history.sqlite3").to_str().unwrap()).unwrap());
    let queue = queue_with_history(&db, history_path.clone());
    let hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    queue.reconcile(vec![queued_upload(hash, TransferOrigin::Background)]);
    let completed = queue.claim_due().unwrap();
    queue.complete(completed.id, None);
    drop(queue);

    let restored = queue_with_history(&db, history_path);
    let history = restored.history();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].state, TransferState::Completed);
    assert_eq!((history[0].completed_items, history[0].total_items), (1, 1));
}

#[test]
fn thumbnail_network_work_is_claimed_in_bounded_batches() {
    let queue = TransferQueue::default();
    let jobs: Vec<_> = (0..20).map(|index| TransferJob { operation: library_runtime::TransferOperation::DownloadThumbnail { thumbnail_hash: fixture_content_hash(index) }, origin: TransferOrigin::Background }).collect();
    queue.reconcile(jobs.clone());
    let first = queue.claim_due_batch().into_iter().map(|claimed| claimed.job).collect::<Vec<_>>();
    let second = queue.claim_due_batch().into_iter().map(|claimed| claimed.job).collect::<Vec<_>>();
    assert_eq!(first, jobs[..MAX_THUMBNAIL_BATCH_ENTRIES]);
    assert_eq!(second, jobs[MAX_THUMBNAIL_BATCH_ENTRIES..]);
}

#[test]
fn reconciliation_removes_obsolete_jobs_from_batch_progress() {
    let queue = TransferQueue::default();
    let first = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let second = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let obsolete = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    queue.reconcile(vec![queued_upload(first, TransferOrigin::Background), queued_upload(second, TransferOrigin::Background), queued_upload(obsolete, TransferOrigin::Background)]);

    let completed = queue.claim_due().unwrap();
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
        TransferJob { operation: library_runtime::TransferOperation::DownloadThumbnail { thumbnail_hash: thumbnail_hash }, origin: TransferOrigin::Background },
    ]);

    let claimed = queue.claim_due().unwrap();
    assert!(matches!(claimed.job.operation, TransferOperation::DownloadThumbnail { .. }));
}

#[test]
fn ephemeral_queue_prioritizes_user_jobs_and_tracks_retries() {
    let queue = TransferQueue::default();
    let background = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let manual = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    queue.reconcile(vec![queued_upload(background, TransferOrigin::Background), queued_upload(manual, TransferOrigin::UserInitiated)]);

    let claimed = queue.claim_due().expect("user transfer has priority");
    assert_eq!(claimed.job.origin(), TransferOrigin::UserInitiated);
    assert!(queue.retry(claimed.id, "network unavailable".to_owned()).is_some());
    let retrying = queue.statuses().into_iter().find(|status| status.content_hash == ContentHash::new(manual)).unwrap();
    assert_eq!(retrying.state, TransferState::Retrying);
    assert_eq!(retrying.attempts, 1);
    assert_eq!(retrying.last_error.as_deref(), Some("network unavailable"));

    let background = queue.claim_due().expect("background transfer proceeds while the user job awaits retry");
    assert_eq!(background.job.origin(), TransferOrigin::Background);
}

#[test]
fn authentication_pause_does_not_consume_retry_budget() {
    let queue = TransferQueue::default();
    let hash = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    queue.reconcile(vec![queued_upload(hash, TransferOrigin::Background)]);
    let rejected = queue.claim_due().unwrap();
    queue.release(std::slice::from_ref(&rejected));
    queue.pause_authentication();

    assert!(!queue.try_start_scheduler(), "authentication pauses the queue rather than changing a job");
    queue.authentication_paused.store(false, std::sync::atomic::Ordering::Release);
    queue.wake_scheduler.notify_one();
    assert!(queue.try_start_scheduler());

    let retried = queue.claim_due().unwrap();
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
fn only_an_active_upload_remains_retryable_after_admission() {
    use sync_common::api::assets::BlobManifestRejectionReason;

    assert!(matches!(TransferError::from_upload_admission(BlobManifestRejectionReason::UploadInProgress), TransferError::Retryable(_)));
    for reason in [BlobManifestRejectionReason::InvalidSize, BlobManifestRejectionReason::SizeMismatch, BlobManifestRejectionReason::IdentityMismatch, BlobManifestRejectionReason::QuotaExceeded] {
        assert!(matches!(TransferError::from_upload_admission(reason), TransferError::Rejected(_)));
    }
}

fn admit_uploads(app: Router) -> Router {
    app.route(
        &format!("/api/libraries/{}/blobs/negotiate", test_support::fixture_library_id("library")),
        axum::routing::post(|body: axum::body::Bytes| async move {
            let request: sync_common::api::assets::BlobManifestRequest = sync_common::transport::decode(&body, sync_common::transport::MAX_DECODED_REQUEST_BYTES).unwrap();
            let response = sync_common::api::assets::BlobManifestResponse { upload: request.blobs.into_iter().map(|blob| blob.content_hash).collect(), ..Default::default() };
            ([("content-type", sync_common::transport::MEDIA_TYPE)], sync_common::transport::encode(&response).unwrap())
        }),
    )
}

#[test]
fn failure_classification_is_closed_and_explicit() {
    assert!(matches!(TransferError::rejected("bad input"), TransferError::Rejected(_)));
    let io_error = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "offline");
    assert!(matches!(TransferError::from(io_error), TransferError::Retryable(_)));
}

#[tokio::test]
async fn permanent_http_asset_errors_are_not_retried() {
    let app = Router::new().route("/rejected", get(|| async { StatusCode::FORBIDDEN }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, admit_uploads(app)).await.unwrap() });

    let response = reqwest::Client::new().get(format!("http://{address}/rejected")).send().await.unwrap();
    let error = crate::validate_streaming_asset_response(response).unwrap_err();
    assert!(matches!(TransferError::from(error), TransferError::Rejected(_)));
    server.abort();
}
