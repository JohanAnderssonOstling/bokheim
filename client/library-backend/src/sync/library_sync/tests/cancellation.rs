use super::*;

#[tokio::test]
async fn sync_activity_clears_on_cancellation_without_hiding_overlapping_work() {
    let count = AtomicUsize::new(0);
    let other = SyncActivity::begin(&count, None);
    let mut work = Box::pin(async {
        let _activity = SyncActivity::begin(&count, None);
        std::future::pending::<()>().await;
    });
    assert!(futures_util::poll!(work.as_mut()).is_pending());
    assert_eq!(count.load(Ordering::Acquire), 2);
    drop(work);
    assert_eq!(count.load(Ordering::Acquire), 1);
    drop(other);
    assert_eq!(count.load(Ordering::Acquire), 0);
}
